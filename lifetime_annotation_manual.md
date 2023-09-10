# Lifetime Annotation Manual
## Overview
Rustviz lifetime visualization feature creates interactive SVG on calculation of lifetime parameters in function signatures and struct methods. If multiple lifetime parameters is present in one function/method, it will group calculation for the same lifetime parameter and render visualization for all lifetime parameter in a unified way. For example, the following functions/methods can be visualized:
+ Lifetime parameters in function call:
```rust
fn max<'a,'b,'r> (lhs: &'a i32, rhs: &'b i32) -> &'r {...}
```
+ Lifetime parameters in struct static method:
```rust
struct Wheel{...};
impl Wheel{
    
}
```