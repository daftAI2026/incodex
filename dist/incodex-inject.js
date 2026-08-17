var F=["[data-app-action-sidebar-thread-row]","[data-app-action-sidebar-project-row]"],P=new Set(["Pinned","Recents"]),f=new Set(["Show more","显示更多","展开显示","Show all","显示全部"]),k=new Set(["No chats","没有聊天"]);var m=new Set(["Search","搜索"]);function A(){try{return window.localStorage.getItem("incodex-privacy")==="1"}catch{return!1}}function U(){return window.__incodexIncognito===!0||window.incodex?.isIncognito?.()===!0}function V(z){return z?"退出无痕":"无痕"}function N(z){document.documentElement.setAttribute("data-incodex-privacy",z?"on":"off");let G=document.querySelector("[data-incodex-privacy-toggle]");if(G)G.setAttribute("aria-pressed",z?"true":"false"),G.setAttribute("aria-label",V(z));let J=document.querySelector("[data-incodex-tooltip-label]");if(J)J.textContent=V(z);if(U())return;O(z)}function O(z){for(let G of document.querySelectorAll("[data-app-action-sidebar-section]")){let J=G.getAttribute("data-app-action-sidebar-section-heading")||"";if(z&&P.has(J))G.setAttribute("data-incodex-empty-section","");else G.removeAttribute("data-incodex-empty-section");for(let K of G.querySelectorAll("button")){let Q=(K.textContent||"").replace(/\s+/g," ").trim();if(!f.has(Q))continue;if(z)K.setAttribute("data-incodex-show-more","");else K.removeAttribute("data-incodex-show-more")}for(let K of G.querySelectorAll("span, div, p")){let Q=(K.textContent||"").replace(/\s+/g," ").trim();if(!k.has(Q))continue;if(z)K.setAttribute("data-incodex-empty-chat","");else K.removeAttribute("data-incodex-empty-chat")}}}async function B(){try{if((await window.incodex?.openIncognito?.())?.ok)return!0}catch{}try{return await fetch("https://incodex.invalid/open",{mode:"no-cors",cache:"no-store"}),!0}catch{return!1}}async function D(){if(X(),U()){try{await window.incodex?.quitIncognito?.()}catch{window.close()}return}await B()}function R(){let z=document.getElementById("incodex-privacy-style");if(!z)z=document.createElement("style"),z.id="incodex-privacy-style",document.head.append(z);let G=F.map((J)=>`html[data-incodex-privacy="on"] ${J}`).join(`,
`);z.textContent=`
    ${G} { display: none !important; }
    html[data-incodex-privacy="on"] [data-incodex-empty-section],
    html[data-incodex-privacy="on"] [data-app-action-sidebar-project-show-all-toggle],
    html[data-incodex-privacy="on"] [data-incodex-show-more],
    html[data-incodex-privacy="on"] [data-incodex-empty-chat] {
      display: none !important;
    }
    [data-incodex-tooltip] {
      position: fixed;
      z-index: 50;
      display: none;
      width: max-content;
      max-width: min(20rem, calc(100vw - 16px));
      pointer-events: none !important;
      user-select: none;
      box-sizing: border-box;
    }
    [data-incodex-tooltip][data-open="true"] { display: block; }
    [data-incodex-landing] {
      pointer-events: none;
      max-width: 28rem;
      margin: 0 auto;
      padding: 0.5rem 1rem 1.5rem;
    }
    [data-incodex-landing] h2 {
      margin: 0 0 0.5rem;
      font-size: 1.25rem;
      font-weight: 600;
      line-height: 1.3;
    }
    [data-incodex-landing] p,
    [data-incodex-landing] li {
      margin: 0;
      font-size: 0.875rem;
      line-height: 1.55;
    }
    [data-incodex-landing] ul {
      margin: 0.75rem 0 0;
      padding-left: 1.1rem;
    }
    [data-incodex-landing] li + li { margin-top: 0.35rem; }
  `}function W(){return[...document.querySelectorAll("button")].find((z)=>m.has((z.getAttribute("aria-label")||"").trim()))??null}function _(z){return z.closest(".ms-auto.flex.items-center")}function x(z,G,J){return z.parentElement===G&&!J.contains(z)&&!z.contains(J)}function C(z){let G=z.cloneNode(!1);G.removeAttribute("id"),G.removeAttribute("aria-haspopup"),G.removeAttribute("aria-expanded"),G.removeAttribute("data-state"),G.setAttribute("type","button"),G.setAttribute("data-incodex-privacy-toggle","true"),G.className=z.className;let J=document.createElement("span");J.innerHTML=`<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
  <path d="M14 18a2 2 0 0 0-4 0"/>
  <path d="m19 11-2.11-6.657a2 2 0 0 0-2.752-1.148l-1.276.61A2 2 0 0 1 12 4H8.5a2 2 0 0 0-1.925 1.456L5 11"/>
  <path d="M2 11h20"/>
  <circle cx="17" cy="18" r="3"/>
  <circle cx="7" cy="18" r="3"/>
</svg>`.trim();let K=J.firstElementChild;if(K){let Q=z.querySelector("svg");K.setAttribute("class",Q?.getAttribute("class")||"icon-xs"),K.setAttribute("aria-hidden","true"),K.setAttribute("width",Q?.getAttribute("width")||"16"),K.setAttribute("height",Q?.getAttribute("height")||"16"),G.append(K)}return G.addEventListener("click",(Q)=>{Q.preventDefault(),Q.stopImmediatePropagation(),D()},!0),G.addEventListener("pointerenter",()=>Z(G)),G.addEventListener("pointerleave",X),G.addEventListener("focus",()=>Z(G)),G.addEventListener("blur",X),G}function H(){let z=document.querySelector("[data-incodex-tooltip]");if(z)return z;z=document.createElement("div"),z.setAttribute("data-incodex-tooltip","true"),z.setAttribute("role","tooltip"),z.className="z-50 w-fit select-none text-sm whitespace-normal break-words rounded-lg border border-text bg-primary-solid text-primary-solid px-2 py-1.5";let G=document.createElement("div");G.className="flex items-center gap-2";let J=document.createElement("div");J.className="min-w-0",J.setAttribute("data-incodex-tooltip-label","true");let K=document.createElement("kbd");return K.className="inline-flex !rounded-md !border-0 !bg-current/10 !font-sans !text-xs !text-current !shadow-none !px-1.5 !py-0.5 !leading-none",K.textContent="⇧⌘N",G.append(J,K),z.append(G),document.body.append(z),z}function Z(z){let G=H(),J=G.querySelector("[data-incodex-tooltip-label]");if(J)J.textContent=V(z.getAttribute("aria-pressed")==="true");G.setAttribute("data-open","true");let K=z.getBoundingClientRect(),Q=G.getBoundingClientRect(),M=Math.min(window.innerWidth-Q.width-8,Math.max(8,K.left+K.width/2-Q.width/2)),Y=Math.max(8,K.top-Q.height-8);G.style.left=`${M}px`,G.style.top=`${Y}px`}function X(){let z=document.querySelector("[data-incodex-tooltip]");if(z)z.removeAttribute("data-open")}function I(){if(document.querySelector("[data-app-action-sidebar-thread-row]"))return!0;if(document.querySelector("[data-message-author-role], [data-turn-id]"))return!0;return!1}function L(){let G=[...document.querySelectorAll("h1, h2")].find((Q)=>/构建什么|What (will|should) you build|我们来构建/.test(Q.textContent||""));if(G?.parentElement)return G.parentElement;return document.querySelector("textarea, [contenteditable='true']")?.closest("main, [class*='flex-1']")??document.querySelector("main")}function T(){let z=document.querySelector("[data-incodex-landing]");if(z)return z;return z=document.createElement("aside"),z.setAttribute("data-incodex-landing","true"),z.className="text-default",z.innerHTML=`
    <h2>这是一个干净窗口</h2>
    <p class="text-codex-description">看不到平时的对话。关掉这个窗口后，这次聊过的内容会从临时目录清掉。原来的窗口还在。</p>
    <ul class="text-codex-description">
      <li>不会写入你平时的会话库</li>
      <li>登录、语言和基础设置跟主窗口一样</li>
      <li>再按 ⇧⌘N，或点「退出无痕」，即可离开</li>
    </ul>
  `,z}function $(){let z=document.querySelector("[data-incodex-landing]");if(!U()||I()){z?.remove();return}let G=L();if(!G)return;let J=T();if(J.parentElement!==G)G.insertBefore(J,G.firstElementChild)}function j(){let z=W();if(!z)return;let G=_(z);if(!G)return;G.setAttribute("data-incodex-header-cluster","true");let J=document.querySelector("[data-incodex-privacy-toggle]");if(!J)J=C(z);if(!x(J,G,z))G.insertBefore(J,G.firstElementChild);N(U()||A())}function w(z){if(!(z.metaKey||z.ctrlKey)||!z.shiftKey)return;if(z.code!=="KeyN"&&z.key.toLowerCase()!=="n")return;z.preventDefault(),z.stopImmediatePropagation(),D()}function q(){if(window.__incodexStarted)return;if(window.__incodexStarted=!0,!U())try{window.localStorage.removeItem("incodex-privacy")}catch{}R(),j(),N(U()),$(),window.addEventListener("keydown",w,!0);let z=!1;new MutationObserver(()=>{if(z)return;z=!0,requestAnimationFrame(()=>{z=!1,j(),$()})}).observe(document.documentElement,{childList:!0,subtree:!0})}if(document.readyState==="loading")document.addEventListener("DOMContentLoaded",q,{once:!0});else q();
