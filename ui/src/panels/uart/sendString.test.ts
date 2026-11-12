import { describe, expect, it } from 'vitest';
import { LARGE_PASTE_BYTES } from '../../sdk';
import { encodeSendString, parseHex, previewText, routeFor } from './sendString';

const hex = (r: ReturnType<typeof encodeSendString>) => (r.ok ? [...r.bytes].map((b) => b.toString(16).padStart(2, '0')).join('') : `ERR:${r.error}`);

describe('Send String encoding', () => {
  it('parses HEX with optional separators and 0x prefixes', () => {
    for (const src of ['54 68', '5468', '54-68', '0x54,0x68', ' 54:68\n']) expect(hex(parseHex(src))).toBe('5468');
  });

  it('rejects odd digit counts and non-hex characters', () => {
    expect(parseHex('5').ok).toBe(false);
    expect(parseHex('GZ').ok).toBe(false);
    expect(parseHex('54 6').ok).toBe(false);
  });

  it('applies the explicit line ending exactly in text mode', () => {
    expect(hex(encodeSendString('text', 'ls', 'cr'))).toBe('6c730d');
    expect(hex(encodeSendString('text', 'ls', 'lf'))).toBe('6c730a');
    expect(hex(encodeSendString('text', 'ls', 'crlf'))).toBe('6c730d0a');
    expect(hex(encodeSendString('text', 'ls', 'none'))).toBe('6c73');
    // every line break in the box uses the same ending (Enter emulation)
    expect(hex(encodeSendString('text', 'a\nb', 'crlf'))).toBe('610d0a620d0a');
    expect(hex(encodeSendString('text', 'a\r\nb', 'none'))).toBe('6162');
  });

  it('encodes text as UTF-8 and appends the ending to HEX input', () => {
    expect(hex(encodeSendString('text', 'é', 'none'))).toBe('c3a9');
    expect(hex(encodeSendString('hex', '1B 5B', 'cr'))).toBe('1b5b0d');
    expect(hex(encodeSendString('hex', '00 FF', 'none'))).toBe('00ff');
  });

  it('refuses empty input (an ending alone is not sent implicitly)', () => {
    expect(encodeSendString('text', '', 'cr').ok).toBe(false);
    expect(encodeSendString('hex', '  ', 'crlf').ok).toBe(false);
  });

  it('routes > LARGE_PASTE_BYTES to the inputLarge confirm card', () => {
    expect(routeFor(LARGE_PASTE_BYTES)).toBe('input');
    expect(routeFor(LARGE_PASTE_BYTES + 1)).toBe('inputLarge');
  });

  it('previews the exact final bytes', () => {
    expect(previewText(new Uint8Array([0x6c, 0x73, 0x0d]))).toBe('3 B · 6C 73 0D');
    expect(previewText(new Uint8Array(30), 2)).toBe('30 B · 00 00 … +28 B');
  });
});
