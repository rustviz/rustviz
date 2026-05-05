# Rust Basics

## Main Function
In every Rust program, the `main` function executes first:
```rust
fn main() {
    // code here will run first
}
```

## Variables
In Rust, we use `let` bindings to introduce variables. Variables are *immutable*
or *mutable*.

### Immutable Variables
By default, variables are *immutable* in Rust. This means that once a value is
bound to the variable, the binding cannot be changed. We use `let` bindings to
introduce immutable variables as follows:
```rust
fn main() {
    let x = 5;
}
```

In this example, we introduce a variable `x` of type `i32` (a 32-bit signed
integer type) and bind the value `5` to it. 

You cannot assign to an immutable variable. So the following example causes a
compiler error:
```rust
fn main() {
    let x = 5;
    x = 6; // ERROR: cannot assign twice to immutable variable x
}
```

### Mutable Variables
If you want to be able to assign to a variable, it must be marked as *mutable*
with `let mut`:
```rust
fn main() {
    let mut x = 5;
    x = 6; //OK
}
```

## Copies
For simple types like integers, binding and assignment creates a copy. 
For example, we can bind the value `5` to `x` and then bind `y` with a copy of `x`:
```rust
fn main() {
    let x = 5;
    let y = x;
}
```

Copying occurs only for simple types like `i32` and other types that
have been marked as copyable (they implement the `Copy` trait -- we will not 
discuss traits here).
We will discuss how more interesting data
structures that are not copyable behave differently in later sections
of the tutorial.

## Functions
Besides `main`, we can define additional functions. In the following example, we
define a function called `plus_one` which takes an `i32` as input and returns an
`i32` value that is one more than the input:
```rust
fn main() {
    let six = plus_one(5);
}

fn plus_one(x: i32) -> i32 {
    x + 1
}
```

Notice how there is no explicit return. In Rust, if the last expression in the
function body does not end in a semicolon, it is the return value. (Rust also
has a `return` keyword, but we do not use it here.)

## Printing to the Terminal
In Rust, we can print to the terminal using `println!`:
```rust
fn main() {
    println!("Hello, world!")
}
```
This code prints `Hello, world!` to the terminal, followed by a newline
character.

We can also use curly brackets in the input string of `println!` as a
placeholder for subsequent arguments:
```rust
fn main() {
    let x = 1;
    let y = 2;
    println!("x = {} and y = {}", x, y);
}
```

This prints `x = 1 and y = 2`.

Note that the `!` at the end of `println!` indicates that it is a *macro*, not a
function. It behaves slightly differently from normal functions, but you do not
need to worry about the details here. 