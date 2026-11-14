// "Open in Playground" links: `https://<playground>/#code=<base64url>`.
//
// The payload is the program's UTF-8 bytes in base64url (RFC 4648 §5:
// `-` and `_` instead of `+` and `/`) with no `=` padding. The
// mdbook-rustviz preprocessor emits these links under every figure
// (see `mdbook-rustviz/src/preprocessor.rs`); keep both ends in sync.

const PREFIX = '#code=';

/** Encode source text as unpadded base64url of its UTF-8 bytes. */
export function encodeCode(code: string): string {
  const bytes = new TextEncoder().encode(code);
  let bin = '';
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

/** Inverse of `encodeCode`; null if the payload is not valid base64url UTF-8. */
export function decodeCode(payload: string): string | null {
  if (!/^[A-Za-z0-9_-]*$/.test(payload) || payload.length % 4 === 1) return null;
  try {
    const std = payload.replace(/-/g, '+').replace(/_/g, '/');
    const bin = atob(std + '='.repeat((4 - (std.length % 4)) % 4));
    const bytes = Uint8Array.from(bin, c => c.charCodeAt(0));
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

/** The program carried by a `location.hash`, or null if absent or malformed. */
export function codeFromHash(hash: string): string | null {
  if (!hash.startsWith(PREFIX)) return null;
  const code = decodeCode(hash.slice(PREFIX.length));
  return code === null || code.trim() === '' ? null : code;
}

export function hashForCode(code: string): string {
  return PREFIX + encodeCode(code);
}
