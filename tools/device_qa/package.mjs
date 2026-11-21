import {cp,mkdir,stat} from 'node:fs/promises';
import path from 'node:path';
const site=path.resolve(process.argv[2]||'target/m9-site');
if(!(await stat(site)).isDirectory())throw Error('Existing exported site required');
await mkdir(path.join(site,'device-qa'),{recursive:true});
for(const name of ['index.html','companion.mjs','recorder.mjs','trace.mjs'])await cp(new URL(name,import.meta.url),path.join(site,'device-qa',name));
console.log(`QA companion packaged without changing player files: ${site}/device-qa/?game=../coin_game/`);
