"use strict";
const http = require("node:http");

function controls(prefix = "") {
  return `<button id="${prefix}click" onclick="this.dataset.count=String(Number(this.dataset.count||0)+1)">Click ${prefix || "DOM"}</button>
<input aria-label="Input ${prefix || "DOM"}" id="${prefix}input">
<input type="password" aria-label="Password ${prefix || "DOM"}" id="${prefix}password">
<label><input type="checkbox" id="${prefix}check">Check ${prefix || "DOM"}</label>
<select aria-label="Select ${prefix || "DOM"}" id="${prefix}select"><option value="a">Alpha</option><option value="b">Beta</option></select>`;
}
function semanticControls(prefix = "") {
  return ["primary", "secondary"].map(area =>
    `<section aria-label="${prefix}${area}-area"><button onclick="this.dataset.count=String(Number(this.dataset.count||0)+1)">Semantic ${prefix || "DOM"}</button></section>`).join("");
}
function page(body) {
  return `<!doctype html><html><head><meta charset="utf-8"><title>Python Preview Lab</title><style>
body{font:16px Arial;margin:24px}section{margin:18px 0}iframe{display:block;width:90%;height:320px;border:2px solid #444;margin:16px 0}
button,input,select{margin:4px;padding:6px}x-panel,x-outer{display:block;padding:12px;border:1px solid #777}
</style></head><body>${body}</body></html>`;
}
async function createFixture({boundaries = false} = {}) {
  let ports;
  function handler(req, res) {
    if (req.url === "/test-selector-engine.js") {
      const fs = require("node:fs"), path = require("node:path");
      res.writeHead(200, {"content-type":"application/javascript; charset=utf-8"});
      res.end(["01-uiaDispatcher.core.js","02-uiaDispatcher.selector-engine.js"]
        .map(file => fs.readFileSync(path.join(__dirname,"../../src/web_assets",file),"utf8")).join("\n"));
      return;
    }
    let body;
    if (req.url.startsWith("/popup")) {
      body = `<h2>Disposable Login Popup</h2>${controls("popup-")}`;
    } else if (req.url.startsWith("/cross-nested")) {
      body = `<h2>Nested Cross Origin Document</h2>${controls("cross-nested-")}`;
    } else if (req.url.startsWith("/return-leaf")) {
      body = `<h2>Cross Frame Returning To Top Origin</h2>${controls("return-")}`;
    } else if (req.url.startsWith("/shadow-leaf")) {
      body = `<h2>Shadow Frame</h2>${controls("shadow-frame-")}`;
    } else if (req.url.startsWith("/duplicate")) {
      body = `<h2>Identical URL Document</h2>${controls("duplicate-")}`;
    } else if (req.url.startsWith("/frame")) {
      body = `<h2>Same Origin Frame</h2>${controls("frame-")}${semanticControls("frame-")}<x-panel id="frame-shadow"></x-panel>
<script>document.querySelector("x-panel").attachShadow({mode:"open"}).innerHTML=${JSON.stringify(controls("frame-shadow-") + semanticControls("frame-shadow-"))}</script>
<iframe id="nested-frame" src="/leaf"></iframe>`;
    } else if (req.url.startsWith("/leaf")) {
      body = `<h2>Nested Frame</h2>${controls("nested-")}`;
    } else if (req.url.startsWith("/cross")) {
      const prefix=req.url.includes("shadow=1")?"shadow-cross-":"cross-";
      body = `<h2>Cross Origin Frame</h2>${controls(prefix)}`;
      if(boundaries && prefix==="cross-")body+=`<x-panel id="cross-shadow"></x-panel>
<script>document.querySelector("x-panel").attachShadow({mode:"open"}).innerHTML=${JSON.stringify(controls("cross-shadow-"))}</script>
<iframe id="cross-nested" src="/cross-nested"></iframe>
<iframe id="return-frame" src="http://127.0.0.1:${ports[0]}/return-leaf"></iframe>`;
    } else {
      body = `<h1>Python Preview Lab</h1><section>${controls()}</section>
<section><x-panel id="shadow"></x-panel></section><section><x-outer id="outer"></x-outer></section>
<section><button id="quote-'&quot;-中文" onclick="this.dataset.count=String(Number(this.dataset.count||0)+1)">Quote target</button></section>
<iframe id="same-frame" src="/frame"></iframe><iframe id="cross-frame" src="http://localhost:${ports[1]}/cross"></iframe>
<script>
document.querySelector("#shadow").attachShadow({mode:"open"}).innerHTML=${JSON.stringify(controls("shadow-") + semanticControls("shadow-"))};
const outer=document.querySelector("#outer").attachShadow({mode:"open"});
outer.innerHTML='<x-panel id="inner"></x-panel>';
outer.querySelector("x-panel").attachShadow({mode:"open"}).innerHTML=${JSON.stringify(controls("nested-shadow-") + semanticControls("nested-shadow-"))};
</script>`;
      if(boundaries)body+=`<button id="open-popup" onclick="const f=document.createElement('iframe');f.id='login-popup';f.src='http://localhost:${ports[1]}/popup';f.style.cssText='position:fixed;top:40px;left:40px;width:75%;height:350px;z-index:1000;background:white';document.body.append(f)">Open login popup</button>
<x-panel id="frame-host"></x-panel><script>
document.querySelector("#frame-host").attachShadow({mode:"open"}).innerHTML=
  '<iframe id="shadow-frame" src="/shadow-leaf"></iframe><iframe id="shadow-cross-frame" src="http://localhost:${ports[1]}/cross?shadow=1"></iframe>';
window.__boundaryEvents=[];
window.addEventListener("message",e=>{
  if(e.data?.kind!=="boundary-proof")return;
  const owners=Array.from(document.querySelectorAll("iframe"));
  const direct=owners.find(f=>f.contentWindow===e.source)?.id||null;
  window.__boundaryEvents.push({...e.data,origin:e.origin,direct});
});
</script>
<iframe id="duplicate-a" src="http://localhost:${ports[1]}/duplicate"></iframe>
<iframe id="duplicate-b" src="http://localhost:${ports[1]}/duplicate"></iframe>`;
    }
    if(boundaries)body+=`<script>
for(const event of ["click","input","change"])document.addEventListener(event,e=>{
  const el=e.composedPath()[0];
  if(el?.id)window.top.postMessage({kind:"boundary-proof",event,id:el.id,value:el.value,count:Number(el.dataset?.count||0)},"*");
});
</script>`;
    res.writeHead(200, {"content-type":"text/html; charset=utf-8"});
    res.end(page(body));
  }
  const servers = [http.createServer(handler), http.createServer(handler)];
  await Promise.all(servers.map(s => new Promise(resolve => s.listen(0, "0.0.0.0", resolve))));
  ports = servers.map(s => s.address().port);
  return {url:`http://127.0.0.1:${ports[0]}/`, ports, close:() => Promise.all(servers.map(s => new Promise(resolve => {
    s.close(resolve);
    s.closeAllConnections();
  })))};
}
module.exports = {createFixture};
if (require.main === module) {
  createFixture().then(f => console.log(JSON.stringify({url:f.url, ports:f.ports})));
}
