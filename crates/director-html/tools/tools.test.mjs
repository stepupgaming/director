import test from 'node:test';
import assert from 'node:assert/strict';
import { argumentsOf, capturedMaskMode, capturedTextDecoration, capturedTransformStyle } from './browser.mjs';
import { pixelDiff } from './parity.mjs';
test('pixel comparison detects a missing visible pixel',()=>{
  const a={width:1,height:1,data:Buffer.from([255,0,0,255])};const b={width:1,height:1,data:Buffer.from([0,0,0,255])};
  assert.deepEqual(pixelDiff(a,b),{mae:85,rmse:Math.sqrt(65025/3),max_channel_error:255,changed_pixels:1,changed_pixel_fraction:1});
});
test('pixel comparison rejects mismatched dimensions',()=>assert.throws(()=>pixelDiff({width:2,height:1},{width:1,height:1}),/dimension/));
test('argument parser preserves values and flags',()=>assert.deepEqual(argumentsOf(['--input','file.html','--capture']),{input:'file.html',capture:true}));
test('capture forwards transform-style only when computed preserve-3d',()=>{
  assert.equal(capturedTransformStyle('preserve-3d'),'preserve-3d');
  assert.equal(capturedTransformStyle('flat'),undefined);
  assert.equal(capturedTransformStyle(''),undefined);
  assert.equal(capturedTransformStyle(undefined),undefined);
});
test('capture forwards solid text-decoration lines',()=>{
  assert.equal(capturedTextDecoration('none'),undefined);
  assert.equal(capturedTextDecoration(''),undefined);
  assert.equal(capturedTextDecoration('  '),undefined);
  assert.equal(capturedTextDecoration('underline'),'underline');
  assert.equal(capturedTextDecoration('overline'),'overline');
  assert.equal(capturedTextDecoration('line-through'),'line-through');
  assert.equal(capturedTextDecoration(' underline overline line-through '),'underline overline line-through');
  assert.deepEqual(capturedTextDecoration('underline blink'),{unsupported:'underline blink'});
});
test('capture forwards luminance masks and refuses unknown modes with an image',()=>{
  assert.equal(capturedMaskMode('luminance','none'),'luminance');
  assert.equal(capturedMaskMode('luminance'),'luminance');
  assert.equal(capturedMaskMode('alpha','url("mask.png")'),undefined);
  assert.equal(capturedMaskMode('alpha'),undefined);
  assert.equal(capturedMaskMode('match-source','url("mask.png")'),undefined);
  assert.equal(capturedMaskMode('match-source'),undefined);
  assert.deepEqual(capturedMaskMode('unknown','url("mask.png")'),{unsupported:true});
  assert.equal(capturedMaskMode('unknown'),undefined);
  assert.equal(capturedMaskMode('unknown','none'),undefined);
});
