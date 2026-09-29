'use strict';
(() => {
  const bundle=JSON.parse(document.getElementById('deck').textContent),deck=bundle.pages,assets=bundle.assets;
  const stage=document.getElementById('stage'), status=document.getElementById('status');
  let index=0, view=null, actions={}, triggers={}, drag=null, media={}, keyframes={}, motions={}, overlays=[], variants=new Map(), history=[], started=performance.now(), frame=0;
  const node=id=>stage.querySelector(`[data-node="${id}"]`);
  const say=text=>{status.textContent=text;};
  function parse(svg){return new DOMParser().parseFromString(svg,'image/svg+xml').documentElement;}
  function stopMedia(){stage.querySelectorAll('video,audio').forEach(m=>m.pause());stage.querySelectorAll('iframe').forEach(m=>m.remove());}
  function fit(){if(!view)return;const room=innerHeight-(document.fullscreenElement?0:100);const scale=Math.min(innerWidth/view.width,Math.max(1,room)/view.height);stage.style.width=`${view.width*scale}px`;stage.style.height=`${view.height*scale}px`;}
  function overlayState(){
    const svg=stage.querySelector('svg');
    for(const id of view.overlays){const el=node(id);if(el){el.style.display=overlays.includes(id)?'':'none';el.setAttribute('aria-hidden',overlays.includes(id)?'false':'true');}}
    for(const id of overlays){const el=node(id);if(el)svg.append(el);}
    for(const el of stage.querySelectorAll('[data-action]')) {const top=overlays.at(-1);el.setAttribute('tabindex',top&& !node(top)?.contains(el)?'-1':'0');}
    stage.querySelectorAll('video,audio').forEach(m=>{if(!m.getClientRects().length)m.pause();});
  }
  function markActions(){
    stage.querySelectorAll('[data-action-hit]').forEach(el=>el.remove());
    stage.querySelectorAll('[data-action]').forEach(el=>{el.removeAttribute('data-action');el.removeAttribute('tabindex');el.removeAttribute('role');});
    for(const id of Object.keys(actions)){for(const el of stage.querySelectorAll(`[data-node="${id}"]`)){el.dataset.action=id;el.setAttribute('role','button');el.setAttribute('tabindex','0');el.setAttribute('aria-label',view.labels?.[id]||`Object ${id}`);el.style.cursor=triggers[id]==='drag_end'?'grab':'pointer';el.style.touchAction=triggers[id]==='drag_end'?'none':'';const box=el.getBBox();if(box.width>0&&box.height>0){const hit=document.createElementNS('http://www.w3.org/2000/svg','rect');hit.dataset.actionHit='';for(const key of ['x','y','width','height'])hit.setAttribute(key,box[key]);hit.setAttribute('fill','transparent');hit.setAttribute('pointer-events','all');el.prepend(hit);}}}
  }
  function installMedia(){
    stage.querySelectorAll('[data-player]').forEach(el=>el.remove());
    for(const [id,item] of Object.entries(media)){
      const parent=node(id);if(!parent)continue;
      const fo=document.createElementNS('http://www.w3.org/2000/svg','foreignObject');fo.dataset.player=id;
      ['x','y','width','height'].forEach((key,i)=>fo.setAttribute(key,item.bounds[i]));
      const host=document.createElementNS('http://www.w3.org/1999/xhtml','div');
      const button=document.createElement('button');button.textContent=item.kind==='youtube'?'Play YouTube video':`Play ${item.name||item.kind}`;
      button.addEventListener('click',event=>{
        event.stopPropagation();
        if(item.kind==='youtube'){
          const player=document.createElement('iframe');player.title='YouTube video';player.allow='autoplay; encrypted-media; picture-in-picture; fullscreen';player.allowFullscreen=true;
          player.src=`https://www.youtube-nocookie.com/embed/${item.video}?start=${item.start}&autoplay=1&rel=0`;
          host.replaceChildren(player);return;
        }
        const player=document.createElement(item.kind);player.controls=true;player.preload='metadata';player.volume=item.volume;player.playsInline=true;
        player.src=`data:${item.mime};base64,${assets[item.asset]}`;
        player.addEventListener('loadedmetadata',()=>{
          const end=item.end===null?player.duration:Math.min(item.end,player.duration);
          if(!Number.isFinite(end)||item.start>=end){host.textContent='Trim start is outside this media duration.';return;}
          player.currentTime=item.start;player.dataset.trimStart=item.start;player.dataset.trimEnd=end;player.dataset.loop=String(item.loop);
          player.play().catch(()=>say('Use the media play control to start playback.'));
        },{once:true});
        player.addEventListener('seeking',()=>{const begin=Number(player.dataset.trimStart);if(Number.isFinite(begin)&&player.currentTime<begin)player.currentTime=begin;});
        player.addEventListener('error',()=>{say('This browser could not decode the media. Check its supported codecs.');});
        player.addEventListener('ended',()=>{if(item.loop){player.currentTime=item.start;player.play().catch(()=>{});}});
        host.replaceChildren(player);
      });
      host.append(button);fo.append(host);parent.append(fo);
    }
  }
  function replaceVariant(id,name){
    const asset=view.variants[`${id}:${name}`],target=node(id);if(!asset||!target){say('This component state is not available at this width.');return false;}
    const svg=parse(asset.svg),replacement=svg.querySelector(`[data-node="${id}"]`);if(!replacement)return false;
    const oldIds=[target,...target.querySelectorAll('[data-node]')].map(n=>n.dataset.node);
    for(const key of oldIds){delete actions[key];delete triggers[key];delete media[key];delete keyframes[key];delete motions[key];}
    const newIds=[replacement,...replacement.querySelectorAll('[data-node]')].map(n=>n.dataset.node);
    for(const key of newIds){if(asset.actions[key])actions[key]=asset.actions[key];if(asset.triggers?.[key])triggers[key]=asset.triggers[key];if(asset.media[key])media[key]=asset.media[key];if(asset.keyframes[key])keyframes[key]=asset.keyframes[key];if(asset.motion[key])motions[key]=asset.motion[key];}
    target.replaceWith(document.importNode(replacement,true));variants.set(String(id),name);return true;
  }
  function render(reset){
    stopMedia();cancelAnimationFrame(frame);
    const views=deck[index].views;view=views.filter(v=>v.width<=innerWidth).at(-1)||views[0];
    stage.replaceChildren(document.importNode(parse(view.svg),true));actions=structuredClone(view.actions);triggers=structuredClone(view.triggers||{});drag=null;media=structuredClone(view.media);keyframes=structuredClone(view.keyframes);motions=structuredClone(view.motion);
    if(reset){overlays=[];variants.clear();started=performance.now();}
    else for(const [id,name] of variants)replaceVariant(id,name);
    markActions();installMedia();overlayState();fit();
    if(reset&&view.transition!=='none'){
      const transforms={slide:'translateX(100%)',slide_left:'translateX(-100%)',slide_up:'translateY(100%)',slide_down:'translateY(-100%)',zoom:'scale(.82)',zoom_out:'scale(1.18)'};
      const from={opacity:view.transition.startsWith('slide')?1:0,transform:transforms[view.transition]||'none'};
      stage.animate([from,{opacity:1,transform:'none'}],{duration:view.transition_ms,easing:'ease-in-out'});
    }
    document.getElementById('label').textContent=`${index+1} / ${deck.length} · ${deck[index].name}`;
    document.title=`${deck[index].name} · Emulsion`;frame=requestAnimationFrame(tick);
  }
  function go(next,remember=true){if(next<0||next>=deck.length||next===index)return;if(remember)history.push(index);index=next;say('');render(true);}
  function run(id){
    const target=node(id),top=overlays.at(-1);if(top&&!node(top)?.contains(target))return;
    for(const action of actions[id]||[]){
      switch(action.type){
        case 'url':if(/^https?:\/\//.test(action.url))window.open(action.url,'_blank','noopener,noreferrer');break;
        case 'next':go(index+1);break;
        case 'previous':go(index-1);break;
        case 'back':if(history.length)go(history.pop(),false);break;
        case 'slide':{const next=deck.findIndex(p=>p.id===action.page);if(next<0)say('The destination slide was not included in this export.');else go(next);break;}
        case 'overlay':{const exists=overlays.includes(action.target);overlays=overlays.filter(id=>id!==action.target);if(action.operation==='show'||(action.operation==='toggle'&&!exists))overlays.push(action.target);overlayState();break;}
        case 'close_overlay':overlays.pop();overlayState();break;
        case 'variant':replaceVariant(action.target,action.variant);markActions();installMedia();overlayState();break;
      }
    }
  }
  function sample(track,time){const frames=track.frames;if(time<=frames[0].time_ms)return frames[0].value;for(let i=1;i<frames.length;i++){const a=frames[i-1],b=frames[i];if(time<=b.time_ms){let t=(time-a.time_ms)/(b.time_ms-a.time_ms);switch(a.easing){case'ease_in':t*=t;break;case'ease_out':t=1-(1-t)**2;break;case'ease_in_out':t=t*t*(3-2*t);break;case'step':t=t<1?0:1;break;}return a.value+(b.value-a.value)*t;}}return frames.at(-1).value;}
  function tick(now){
    const time=Math.min(view.duration,Math.max(0,now-started));
    for(const id of new Set([...Object.keys(keyframes),...Object.keys(motions)])){
      const tracks=keyframes[id]||[];
      const el=node(id);if(!el)continue;const values={translation_x:0,translation_y:0,rotation:0,scale_x:1,scale_y:1,opacity:1,visibility:1};
      for(const track of tracks)values[track.property]=sample(track,time);
      const motion=motions[id];
      if(motion){
        el.style.visibility=time<motion.start_ms||time>motion.end_ms?'hidden':'';
        for(const [effect,p,direction] of [[motion.enter,(time-motion.start_ms)/motion.transition_ms,1],[motion.exit,(motion.end_ms-time)/motion.transition_ms,-1]]) {
          let t=Math.max(0,Math.min(1,p));t=t*t*(3-2*t);
          if(effect==='fade')values.opacity*=t;
          if(effect==='slide'){values.translation_x+=motion.offset[0]*(1-t)*direction;values.translation_y+=motion.offset[1]*(1-t)*direction;}
          if(effect==='zoom'){values.scale_x*=0.8+0.2*t;values.scale_y*=0.8+0.2*t;}
        }
      }
      if(values.visibility<0.5)el.style.visibility='hidden';else if(!motion)el.style.visibility='';
      el.style.transformBox='fill-box';el.style.transformOrigin='center';el.style.transform=`translate(${values.translation_x}px,${values.translation_y}px) rotate(${values.rotation}deg) scale(${values.scale_x},${values.scale_y})`;
      el.style.opacity=String(Number(el.getAttribute('opacity')||1)*values.opacity);
    }
    for(const m of stage.querySelectorAll('video,audio')){const end=Number(m.dataset.trimEnd);if(Number.isFinite(end)&&m.currentTime>=end){if(m.dataset.loop==='true'){m.currentTime=Number(m.dataset.trimStart);if(m.paused)m.play().catch(()=>{});}else{m.pause();if(m.currentTime>end)m.currentTime=end;}}}
    frame=requestAnimationFrame(tick);
  }
  stage.addEventListener('click',event=>{if(event.target.closest('[data-player]'))return;const el=event.target.closest('[data-action]');if(el&&(triggers[el.dataset.action]||'click')==='click')run(el.dataset.action);});
  stage.addEventListener('pointerover',event=>{
    if(event.target.closest('[data-player]'))return;const el=event.target.closest('[data-action]');if(!el)return;
    if(triggers[el.dataset.action]==='hover'&&event.relatedTarget?.closest?.('[data-action]')?.dataset.action!==el.dataset.action)run(el.dataset.action);
  });
  stage.addEventListener('pointerdown',event=>{
    if(event.button!==0||event.target.closest('[data-player]'))return;const el=event.target.closest('[data-action]');
    if(el&&triggers[el.dataset.action]==='drag_end'){drag={id:el.dataset.action,pointer:event.pointerId,x:event.clientX,y:event.clientY};stage.setPointerCapture(event.pointerId);event.preventDefault();}
  });
  stage.addEventListener('pointerup',event=>{
    if(!drag||drag.pointer!==event.pointerId)return;const finished=drag;drag=null;if(stage.hasPointerCapture(event.pointerId))stage.releasePointerCapture(event.pointerId);
    if(Math.hypot(event.clientX-finished.x,event.clientY-finished.y)>=4)run(finished.id);
  });
  stage.addEventListener('pointercancel',()=>{drag=null;});
  stage.addEventListener('lostpointercapture',()=>{drag=null;});
  document.addEventListener('keydown',event=>{
    if(event.target.closest('video,audio,iframe,input,textarea'))return;
    if(event.key==='Escape'&&overlays.length){event.preventDefault();overlays.pop();overlayState();return;}
    if((event.key==='Enter'||event.key===' ')&&event.target.dataset.action){event.preventDefault();run(event.target.dataset.action);return;}
    if(event.key==='ArrowRight'||event.key==='PageDown'){event.preventDefault();go(index+1);}
    if(event.key==='ArrowLeft'||event.key==='PageUp'){event.preventDefault();go(index-1);}
    if(event.key.toLowerCase()==='f'&&!event.ctrlKey&&!event.metaKey)fullscreen();
  });
  async function fullscreen(){try{if(document.fullscreenElement)await document.exitFullscreen();else await document.documentElement.requestFullscreen();}catch{say('Fullscreen is unavailable in this browser.');}}
  document.getElementById('prev').onclick=()=>go(index-1);document.getElementById('next').onclick=()=>go(index+1);
  document.getElementById('fullscreen').onclick=fullscreen;document.getElementById('restart').onclick=()=>{started=performance.now();};
  document.addEventListener('fullscreenchange',()=>{document.body.classList.toggle('fullscreen',!!document.fullscreenElement);fit();});
  let resize;addEventListener('resize',()=>{clearTimeout(resize);resize=setTimeout(()=>render(false),100);});
  render(true);
})();
