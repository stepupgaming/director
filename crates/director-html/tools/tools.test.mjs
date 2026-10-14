import test from 'node:test';
import assert from 'node:assert/strict';
import { argumentsOf } from './browser.mjs';
import { pixelDiff } from './parity.mjs';
test('pixel comparison detects a missing visible pixel',()=>{
  const a={width:1,height:1,data:Buffer.from([255,0,0,255])};const b={width:1,height:1,data:Buffer.from([0,0,0,255])};
  assert.deepEqual(pixelDiff(a,b),{mae:85,rmse:Math.sqrt(65025/3),max_channel_error:255,changed_pixels:1,changed_pixel_fraction:1});
});
test('pixel comparison rejects mismatched dimensions',()=>assert.throws(()=>pixelDiff({width:2,height:1},{width:1,height:1}),/dimension/));
test('argument parser preserves values and flags',()=>assert.deepEqual(argumentsOf(['--input','file.html','--capture']),{input:'file.html',capture:true}));
