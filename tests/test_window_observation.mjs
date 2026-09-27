import assert from 'node:assert/strict';
import {decodeControlFrame, encodeControlFrame} from '../extensions/gnome/extension/controlCodec.js';

const frame = {
    type: 'window-observation-config',
    geometry: {x: 0, y: 0, width: 1920, height: 1080},
    config: {generation: 0, applicationIds: [], titles: []},
};
assert.deepEqual(decodeControlFrame(encodeControlFrame(frame)), frame);
for (const config of [
    {generation: -1, applicationIds: [], titles: []},
    {generation: 1.5, applicationIds: [], titles: []},
    {generation: Number.MAX_SAFE_INTEGER + 1, applicationIds: [], titles: []},
    {generation: 1, applicationIds: Array(65).fill('cat'), titles: []},
    {generation: 1, applicationIds: ['猫'.repeat(86)], titles: []},
    {generation: 1, applicationIds: [], titles: ['a\0b']},
    {generation: 1, applicationIds: [], titles: [123]},
]) {
    assert.throws(() => encodeControlFrame({...frame, config}));
}
const unicode = {...frame, config: {generation: 1, applicationIds: ['cat'], titles: ['猫']}};
assert.deepEqual(decodeControlFrame(encodeControlFrame(unicode)), unicode);
const patterns = {...frame, config: {
    generation: 2, applicationIds: ['cat*'], titles: [],
    applicationIdPatterns: ['cat*'], titlePatterns: ['时钟?'],
}};
assert.deepEqual(decodeControlFrame(encodeControlFrame(patterns)), patterns);
for (const changes of [
    {applicationIdPatterns: Array(64).fill('cat*')},
    {titlePatterns: ['a\0b']},
    {titlePatterns: ['猫'.repeat(86)]},
    {titlePatterns: [123]},
    {titlePatterns: null},
]) {
    assert.throws(() => encodeControlFrame({...patterns, config: {...patterns.config, ...changes}}));
}
