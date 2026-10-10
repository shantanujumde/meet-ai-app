/**
 * A fresh, mutable copy of a generated `as const` value (TUR-173).
 *
 * `bindings.ts` writes Rust's defaults as `as const` literals, so their arrays
 * are `readonly` and do not fit the generated types' plain arrays. A copy also
 * means a caller that edits the answer never edits the shared default.
 */

/** `T` with every `readonly` taken off, all the way down. */
export type Writable<T> = T extends readonly (infer U)[]
  ? Writable<U>[]
  : T extends object
    ? { -readonly [K in keyof T]: Writable<T[K]> }
    : T;

export function writable<T>(value: T): Writable<T> {
  return structuredClone(value) as Writable<T>;
}
