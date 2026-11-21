const $=id=>document.getElementById(id),frame=$('game'),status=$('status');
const labels={coldStart:'Kaltstart',firstTap:'Erste Aktivierung',touchWin:'Touch-Sieg',pause:'Pause friert Simulation ein',restart:'Neustart',rotation:'Hoch-/Querformat'};
for(const [id,label] of Object.entries(labels)) {
  const row=document.createElement('label');row.textContent=label;
  const select=document.createElement('select');select.id=id;
  for(const [value,text] of [['not_checked','Nicht geprüft'],['pass','Bestanden'],['fail','Fehlgeschlagen']]){const option=document.createElement('option');option.value=value;option.textContent=text;select.append(option);}
  row.append(select);$('checks').append(row);
}
let report,url,port;
$('toggle').onclick=()=>{
  $('panel').hidden=!$('panel').hidden;
  $('toggle').textContent=$('panel').hidden?'QA anzeigen':'Spiel anzeigen';
  $('toggle').setAttribute('aria-expanded',String(!$('panel').hidden));
};
function prepare() {
  if(!report)return;
  report.owner={physicalDeviceAttested:$('physical').checked,notes:$('notes').value,checklist:Object.fromEntries(Object.keys(labels).map(key=>[key,$(key).value]))};
  if(url)URL.revokeObjectURL(url);
  url=URL.createObjectURL(new Blob([JSON.stringify(report,null,2)],{type:'application/json'}));
  $('download').href=url;$('download').hidden=false;$('share').disabled=false;
}
document.querySelectorAll('input,select,textarea').forEach(element=>element.addEventListener('change',prepare));
addEventListener('message',event=>{
  if(event.origin!==location.origin||event.source!==frame.contentWindow)return;
  if(event.data?.type==='device-qa-ready'&&event.ports[0]){
    port=event.ports[0];
    port.onmessage=event=>{
      if(event.data?.type!=='device-qa-report')return;
      report=event.data.value;prepare();$('stop').disabled=true;status.textContent=`Bericht bereit: ${report.trace.calls.length} Aufrufe; ${report.trace.complete?'vollständiger Tick-Anfang':'unvollständig / keine Ticks'}. Vor dem Teilen prüfen.`;
    };
    status.textContent='Recorder bereit. Oben Spiel anzeigen wählen, dann im Spiel auf Spielen tippen. QA anzeigen öffnet diesen Bericht wieder.';$('stop').disabled=false;$('toggle').disabled=false;
  }
  if(event.data?.type==='device-qa-error')status.textContent=`QA-Fehler: ${event.data.value}. Bericht ist möglicherweise unvollständig.`;
});
$('stop').onclick=()=>port?.postMessage({type:'device-qa-stop'});
$('restart').onclick=()=>location.reload();
$('share').onclick=async()=>{
  const file=new File([JSON.stringify(report,null,2)],'device-qa.json',{type:'application/json'});
  if(!navigator.canShare?.({files:[file]})){status.textContent='Datei-Teilen hier nicht verfügbar. Bitte JSON herunterladen und manuell senden.';return;}
  try{await navigator.share({files:[file],title:'AI Engine Geräte-QA'});}catch(error){status.textContent=error.name==='AbortError'?'Teilen abgebrochen. JSON bleibt verfügbar.':`Teilen fehlgeschlagen: ${error.name}. JSON herunterladen.`;}
};
try {
  const game=new URL(new URLSearchParams(location.search).get('game')||'../coin_game/',location.href);
  if(game.origin!==location.origin||game.search||game.hash||!game.pathname.endsWith('/')||game.pathname===new URL('.',location.href).pathname)throw Error('Nur ein Spielverzeichnis derselben Origin ohne Query/Fragment ist erlaubt');
  frame.addEventListener('load',()=>{
    try{const script=frame.contentDocument.createElement('script');script.type='module';script.src=new URL('./recorder.mjs',location.href).href;frame.contentDocument.head.append(script);}catch{status.textContent='Recorder nicht erreichbar: gleicher Origin und erlaubte Module erforderlich.';}
  });
  frame.src=game.href;
}catch(error){status.textContent=error.message;}
