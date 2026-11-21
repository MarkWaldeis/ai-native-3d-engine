# three.js 0.160.1

`three.module.js`, GLTFLoader, BufferGeometryUtils und die Render-Addons stammen
aus dem npm-Paket `three@0.160.1` (MIT, siehe LICENSE.three). Addons behalten ihre
relativen Importpfade; `three` wird per lokaler Importmap in index.html aufgelöst.
Sky, CSM, Composer, GTAO, Bloom, SMAA, Bokeh und Output samt transitiven Shadern
sind unverändert vendort. Keine CDN-Abhängigkeiten.
