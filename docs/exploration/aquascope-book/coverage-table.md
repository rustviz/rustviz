| kind | chapter | total | renders | refused | compile-error (intended) | compile-error (fragment) | compile-error (needs deps/modules) | compile-error (other) |
|---|---|---|---|---|---|---|---|---|
| aquascope | ch04 | 55 | 31 | 0 | 24 | 0 | 0 | 0 |
| aquascope | ch05 | 15 | 8 | 0 | 7 | 0 | 0 | 0 |
| aquascope | ch06 | 7 | 6 | 0 | 1 | 0 | 0 | 0 |
| aquascope | ch08 | 4 | 2 | 0 | 2 | 0 | 0 | 0 |
| aquascope | expe | 1 | 1 | 0 | 0 | 0 | 0 | 0 |
| listing | ch04 | 4 | 3 | 0 | 1 | 0 | 0 | 0 |
| listing | ch05 | 19 | 15 | 0 | 3 | 1 | 0 | 0 |
| listing | ch06 | 21 | 17 | 2 | 2 | 0 | 0 | 0 |
| listing | ch08 | 29 | 27 | 0 | 2 | 0 | 0 | 0 |
| listing | ch10 | 36 | 23 | 0 | 8 | 2 | 3 | 0 |
| listing | ch13 | 22 | 14 | 0 | 4 | 0 | 4 | 0 |
| listing | ch15 | 30 | 23 | 0 | 7 | 0 | 0 | 0 |
| listing | ch16 | 17 | 10 | 0 | 7 | 0 | 0 | 0 |
| **aquascope total** | | 82 | 48 | 0 | 34 | 0 | 0 | 0 |
| **listing total** | | 178 | 132 | 2 | 34 | 3 | 7 | 0 |

- compile-error (fragment): ch05__listing-05-14  -- error[E0422]: cannot find struct, variant or union type `Rectangle` in this scope
- refused: ch06__listing-06-02  -- RustViz can't visualize this program yet (internal error at plugin.rs:364: an internal lookup found nothing where RustViz expected something). Please report it at https://github.com/rustviz/rustviz/issues with your snippet.
- refused: ch06__no-listing-05-methods-on-enums  -- RustViz can't visualize this program yet (internal error at plugin.rs:364: an internal lookup found nothing where RustViz expected something). Please report it at https://github.com/rustviz/rustviz/issues with your snippet.
- compile-error (fragment): ch10__listing-10-19  -- error[E0425]: cannot find function `longest` in this scope
- compile-error (needs deps/modules): ch10__no-listing-01-calling-trait-method  -- error[E0432]: unresolved import `aggregator`
- compile-error (needs deps/modules): ch10__no-listing-02-calling-default-impl  -- error[E0432]: unresolved import `aggregator`
- compile-error (needs deps/modules): ch10__no-listing-03-default-impl-calls-other-methods  -- error[E0432]: unresolved import `aggregator`
- compile-error (fragment): ch10__no-listing-07-where-clause  -- error[E0405]: cannot find trait `Display` in this scope
- compile-error (needs deps/modules): ch13__listing-12-23-reproduced  -- error[E0432]: unresolved import `minigrep`
- compile-error (needs deps/modules): ch13__listing-12-24-reproduced  -- error[E0432]: unresolved import `minigrep`
- compile-error (needs deps/modules): ch13__listing-13-20  -- error[E0432]: unresolved import `minigrep`
- compile-error (needs deps/modules): ch13__listing-13-22  -- error[E0432]: unresolved import `minigrep`
