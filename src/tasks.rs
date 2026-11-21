use crate::Engine;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn integer(args: &Value, key: &str) -> Result<u64, String> {
    args[key]
        .as_u64()
        .ok_or_else(|| format!("{key} must be a nonnegative integer"))
}

impl Engine {
    fn task_view(&self, task: &Value) -> Value {
        let mut task = task.clone();
        if task["status"] == "claimed" {
            let remaining = self
                .swarm
                .task_leases
                .get(&task["id"].as_u64().unwrap())
                .map(|expiry| expiry.saturating_duration_since(Instant::now()))
                .unwrap_or_default();
            task["ttl_s"] = json!(remaining.as_secs_f64());
            if remaining.is_zero() {
                task["status"] = json!("expired");
            }
        }
        task
    }

    pub(crate) fn task_tool(
        &mut self,
        name: &str,
        args: &Value,
        agent: &str,
    ) -> Result<Value, String> {
        if name == "get_task_changes_since" {
            let sequence = integer(args, "sequence")?;
            return Ok(
                json!({"entries":self.swarm.task_journal.iter().filter(|e|e["sequence"].as_u64().unwrap()>sequence).collect::<Vec<_>>(),"current_sequence":self.swarm.task_sequence,"oldest_sequence":self.swarm.task_journal.front().map(|e|&e["sequence"])}),
            );
        }
        if name == "task_list" {
            if args.get("status").is_some_and(|v| {
                !["pending", "claimed", "completed", "expired"]
                    .iter()
                    .any(|s| v == s)
            }) {
                return Err("status must be pending, claimed, completed, or expired".into());
            }
            return Ok(json!(self
                .swarm
                .tasks
                .iter()
                .map(|t| self.task_view(t))
                .filter(|t| args.get("status").is_none_or(|s| &t["status"] == s))
                .collect::<Vec<_>>()));
        }
        let mut previous_agent = Value::Null;
        let (index, event) = if name == "task_create" {
            let title = args["title"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or("'title' must be a nonempty string")?;
            let deps: Vec<u64> = args
                .get("depends_on")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            for dep in &deps {
                if !self.swarm.tasks.iter().any(|t| t["id"] == *dep) {
                    return Err(format!("no task {dep}; use task_list"));
                }
            }
            if args.get("description").is_some_and(|v| !v.is_string()) {
                return Err("'description' must be a string".into());
            }
            let index = self.swarm.tasks.len();
            self.swarm.tasks.push(json!({"id":index+1,"title":title,"description":args.get("description").cloned().unwrap_or(json!("")),"depends_on":deps,"status":"pending","agent":null,"lease":0}));
            (index, "create")
        } else {
            let id = integer(args, "id")?;
            let index = self
                .swarm
                .tasks
                .iter()
                .position(|t| t["id"] == id)
                .ok_or_else(|| format!("no task {id}; use task_list"))?;
            let task = &self.swarm.tasks[index];
            let live = self
                .swarm
                .task_leases
                .get(&id)
                .is_some_and(|expiry| *expiry > Instant::now());
            let event = if name == "task_claim" {
                if task["status"] != "pending" && !(task["status"] == "claimed" && !live) {
                    return Err(format!(
                        "task {id}: only pending or expired tasks can be claimed"
                    ));
                }
                for dep in task["depends_on"].as_array().unwrap() {
                    if !self
                        .swarm
                        .tasks
                        .iter()
                        .any(|t| t["id"] == *dep && t["status"] == "completed")
                    {
                        return Err(format!("dependency {dep} is not completed"));
                    }
                }
                previous_agent = task["agent"].clone();
                if task["status"] == "claimed" {
                    "takeover"
                } else {
                    "claim"
                }
            } else {
                if task["status"] != "claimed" || task["agent"] != agent {
                    return Err(format!(
                        "task {id} must be claimed by {agent} before {name}"
                    ));
                }
                if !live {
                    return Err(format!("task {id} claim expired; claim again"));
                }
                let lease = task["lease"].as_u64().unwrap();
                if args.get("lease").is_some() {
                    if integer(args, "lease")? != lease {
                        return Err("stale task lease".into());
                    }
                } else if lease != 1 || name != "task_complete" {
                    return Err("lease required; use the latest task_claim lease".into());
                }
                match name {
                    "task_complete" => "complete",
                    "task_renew" => "renew",
                    "task_release" => "release",
                    _ => unreachable!(),
                }
            };
            if matches!(name, "task_claim" | "task_renew") {
                let ttl = args
                    .get("ttl_s")
                    .map(|_| integer(args, "ttl_s"))
                    .transpose()?
                    .unwrap_or(60);
                if ttl == 0 || ttl > 86400 {
                    return Err("ttl_s must be in 1..=86400".into());
                }
                self.swarm
                    .task_leases
                    .insert(id, Instant::now() + Duration::from_secs(ttl));
            }
            if name == "task_claim" {
                let lease = self.swarm.tasks[index]["lease"].as_u64().unwrap() + 1;
                self.swarm.tasks[index]["lease"] = json!(lease);
                self.swarm.tasks[index]["agent"] = json!(agent);
                self.swarm.tasks[index]["status"] = json!("claimed");
            } else if name == "task_complete" {
                if args.get("note").is_some_and(|v| !v.is_string()) {
                    return Err("'note' must be a string".into());
                }
                self.swarm.tasks[index]["status"] = json!("completed");
                self.swarm.tasks[index]["note"] = args.get("note").cloned().unwrap_or(json!(""));
                self.swarm.task_leases.remove(&id);
            } else if name == "task_release" {
                self.swarm.tasks[index]["status"] = json!("pending");
                self.swarm.tasks[index]["agent"] = Value::Null;
                self.swarm.task_leases.remove(&id);
            }
            (index, event)
        };
        let task = self.task_view(&self.swarm.tasks[index]);
        self.swarm.task_sequence += 1;
        self.swarm.task_journal.push_back(json!({"sequence":self.swarm.task_sequence,"agent":agent,"tool":name,"event":event,"task_id":task["id"],"lease":task["lease"],"previous_agent":previous_agent,"status":task["status"],"ttl_s":task.get("ttl_s")}));
        while self.swarm.task_journal.len() > 1024 {
            self.swarm.task_journal.pop_front();
        }
        Ok(task)
    }
}
