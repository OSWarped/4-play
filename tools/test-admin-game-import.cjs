const {test}=require('node:test');
const assert=require('node:assert/strict');
const {matchFile,CHUNK}=require('../control-plane/server/static/admin/game-import.js');
test('matches only expected missing archives; ignores unrelated DAT/artwork and installed BIOS',()=>{
  const options=[{path:'aof.zip',kind:'rom'},{path:'neogeo.zip',kind:'rom',installed:true}];
  assert.equal(matchFile({name:'AOF.ZIP'},options),options[0]);
  for(const name of ['Mame.dat','aof.png','neogeo.zip','mk.zip']) assert.equal(matchFile({name},options),null);
  assert.equal(CHUNK,4*1024*1024);
});
test('CHDs route by expected disk name and machine folder, never arbitrary browser paths',()=>{
  const options=[{path:'kinst/kinst.chd',kind:'chd'},{path:'parent/disk.chd',kind:'chd'},{path:'clone/disk.chd',kind:'chd'}];
  assert.equal(matchFile({name:'kinst.chd'},options),options[0]);
  assert.equal(matchFile({name:'disk.chd'},options),null);
  assert.equal(matchFile({name:'disk.chd',webkitRelativePath:'Fighters/clone/disk.chd'},options),options[2]);
});
test('WAV sample packs cannot be confused with game/device archives of the same name',()=>{
  const options=[{path:'game.zip',kind:'rom'},{path:'game.zip',kind:'samples'}];
  assert.equal(matchFile({name:'game.zip'},options),options[0]);
  assert.equal(matchFile({name:'game.zip'},options,'samples'),options[1]);
  assert.equal(matchFile({name:'game.zip',webkitRelativePath:'collection/samples/game.zip'},options),options[1]);
});

test('shipped upload flow resumes chunks, waits for audit and requires explicit install confirmation',async()=>{
  const fs=require('node:fs'),vm=require('node:vm');
  const elements=new Map(),requests=[],storage=new Map(),timers=[];
  let confirmed=false,refreshed=0;
  const element=id=>{if(!elements.has(id))elements.set(id,{value:'',disabled:false,textContent:'',listeners:{},addEventListener(name,fn){this.listeners[name]=fn;},replaceChildren(){}});return elements.get(id);};
  const panel={querySelector:selector=>element(selector)};
  let job={id:'job1',state:'draft',plan:{game:'aof',title:'Art of Fighting',driver_status:'good',files:[{path:'aof.zip',kind:'rom'}]},uploads:[{id:'f1',path:'aof.zip',kind:'rom',size:8,received:4}]};
  const request=async(path,body,method)=>{
    requests.push({path,body,method});
    if(path==='/api/v1/admin/game-imports' && !body)return {enabled:true,host_id:'linux'};
    if(path.endsWith('/files/f1/4')){assert.ok(body instanceof Blob);assert.equal(body.size,4);return {received:8};}
    if(path.endsWith('/audit'))job.state='auditing';
    if(path.endsWith('/install'))job.state='installing';
    return structuredClone(job);
  };
  const root={confirm:()=>confirmed,document:{createElement:()=>({})},setTimeout:fn=>timers.push(fn)};
  const context=vm.createContext({window:root,localStorage:{getItem:k=>storage.get(k)||null,setItem:(k,v)=>storage.set(k,v),removeItem:k=>storage.delete(k)},setTimeout:fn=>{timers.push(fn);return timers.length;},clearTimeout(){}});
  vm.runInContext(fs.readFileSync('control-plane/server/static/admin/game-import.js','utf8'),context);
  await root.FourPlayGameImport.bind({panel,request,refreshed:()=>refreshed++})();
  element('#game-import-name').value='aof';
  await element('#game-import-inspect').listeners.click();
  const file={name:'aof.zip',size:8,slice:(start,end)=>new Blob([new Uint8Array(Math.min(end,8)-start)])};
  element('#game-import-files').listeners.change({target:{files:[file]}});
  await element('#game-import-upload').listeners.click();
  assert.ok(requests.some(r=>r.path.endsWith('/files/f1/4')&&r.method==='PUT'));
  assert.equal(element('#game-import-install').disabled,true);
  job.state='ready';await timers.shift()();
  assert.equal(element('#game-import-install').disabled,false);
  await element('#game-import-install').listeners.click();
  assert.ok(!requests.some(r=>r.path.endsWith('/install')));
  confirmed=true;await element('#game-import-install').listeners.click();
  assert.ok(requests.some(r=>r.path.endsWith('/install')));
  job.state='installed';await timers.shift()();
  assert.equal(refreshed,1);
});
