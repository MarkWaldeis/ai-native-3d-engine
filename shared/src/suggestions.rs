pub fn hint<'a>(input: &str, choices: impl IntoIterator<Item = &'a str>) -> String {
    if input.len() > 128 {
        return String::new();
    }
    let input = input.to_lowercase();
    let mut ranked = choices
        .into_iter()
        .filter(|s| s.len() <= 128)
        .map(|candidate| {
            let target: Vec<_> = candidate.to_lowercase().chars().collect();
            let mut row: Vec<_> = (0..=target.len()).collect();
            for (i, a) in input.chars().enumerate() {
                let mut diagonal = row[0];
                row[0] = i + 1;
                for (j, b) in target.iter().enumerate() {
                    let old = row[j + 1];
                    row[j + 1] = (row[j] + 1)
                        .min(old + 1)
                        .min(diagonal + usize::from(a != *b));
                    diagonal = old;
                }
            }
            (*row.last().unwrap(), candidate)
        })
        .collect::<Vec<_>>();
    ranked.sort_unstable();
    match ranked.first() {
        Some((distance, name)) if *distance <= 3.min((input.chars().count() / 3).max(1)) => {
            format!("; did you mean '{name}'?")
        }
        _ => String::new(),
    }
}
