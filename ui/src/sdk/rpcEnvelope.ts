// Domain RPC result envelope (loom_proto::Envelope, gateway daemon `envelope()`), shared by
// PanelContext.query (unwraps `data`) and ActionGate (keeps the success shape, throws on failure).
// Core RPCs and test mocks answer bare; those pass through untouched.

/** A domain RPC answered with a failure envelope (`ok:false`, loom_proto::ErrorBody). */
export class DomainRpcError extends Error {
  /** ErrorBody.code (`usage`, `denied`, `stop_latched`, `partial`, …); `internal` when the body is missing */
  readonly code: string;
  readonly hint: string | null;
  readonly retryable: boolean;
  readonly requestId: string | null;
  readonly detail: unknown;
  /** partial `data` some failures still carry (e.g. `partial`); never treated as success */
  readonly data: unknown;
  constructor(rpc: string, env: Record<string, unknown>) {
    const e = isPlainObject(env.error) ? env.error : {};
    super(`${rpc}: ${typeof e.message === 'string' ? e.message : 'domain call failed (no error body)'}`);
    this.name = 'DomainRpcError';
    this.code = typeof e.code === 'string' ? e.code : 'internal';
    this.hint = typeof e.hint === 'string' ? e.hint : null;
    this.retryable = e.retryable === true;
    this.requestId = typeof e.request_id === 'string' ? e.request_id : null;
    this.detail = e.detail;
    this.data = env.data;
  }
}

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

/**
 * True only when every always-serialized Envelope field is present, so a business payload with
 * its own `ok`/`data` (e.g. `eth.filter.check` → `{ok,pos}`) is never mistaken for one.
 */
export function isDomainEnvelope(r: unknown): r is Record<string, unknown> & { ok: boolean; data?: unknown } {
  return (
    isPlainObject(r) &&
    typeof r.schema_version === 'number' &&
    typeof r.ok === 'boolean' &&
    typeof r.cmd === 'string' &&
    isPlainObject(r.meta) &&
    isPlainObject(r.others_since_last)
  );
}

/** Throws DomainRpcError for a failure envelope (partial data included); otherwise returns `r` as is. */
export function rejectDomainFailure<R>(rpc: string, r: unknown): R {
  if (isDomainEnvelope(r) && !(r.ok && r.error == null)) throw new DomainRpcError(rpc, r);
  // RPC boundary: the caller's declared R is trusted, as with net.call
  return r as R;
}

/** Success envelope → its `data`; failure → DomainRpcError; bare result → unchanged. Never mutates `r`. */
export function unwrapDomainResult<R>(rpc: string, r: unknown): R {
  const v = rejectDomainFailure<unknown>(rpc, r);
  return (isDomainEnvelope(v) ? v.data : v) as R;
}
