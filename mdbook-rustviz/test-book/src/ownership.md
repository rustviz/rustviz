# Ownership

In the previous section, we considered only simple values, like integers.
However, in real-world Rust programs, we work with more complex data structures that
allocate resources on the heap. When we allocate resources, we need a strategy
for de-allocating these resources. Most programming languages use one of two
strategies:

1. Manual Deallocation (C, C++): The programmer is responsible for explicitly
deallocating memory, e.g. using `free` in C or `delete` in C++. This is
performant but can result in critical memory safety issues such as use-after-free bugs,
double-free bugs, and memory leaks. These can cause crashes, memory corruption, and 
security vulnerabilities. In fact, about 70% of security bugs in major software 
products like Windows and Chrome are due to memory safety issues.

2. Garbage Collection (OCaml, Java, Python, etc.): The programmer does not have to
explicitly deallocate memory. Instead, a *garbage collector* frees (deallocates)
memory by doing a dynamic analysis that detects when no further references to the data remain
live. 
This prevents memory
safety bugs. However, a garbage collector can incur sometimes substantial run-time
performance overhead.

Rust uses a third strategy—a static (i.e. compile-time) ownership system.
Because this is a purely compile-time mechanism, it achieves memory safety
without the performance overhead of garbage collection!

The key idea is that each resource in memory has a unique *owner*,
which controls access to that resource. When the
owner's lifetime ends (it "dies"), e.g. by going out of scope, 
the resource is deallocated (in Rust,
we say that the resource is *dropped*.)

## Heap-Allocated Strings

For example, heap-allocated strings, of type `String`, are managed by Rust's ownership system.
Consider the following example, which constructs a heap-allocated string and
prints it out.

```rv
fn main() {
    let s = String::from("hello");
    println!("{}", s);
}
```

This code prints `hello`.

The `String::from` function allocates a `String` on the heap. The `String` is
initialized from a provided string literal (string literals themselves have a
more primitive type, `&str`, but that detail is not important here.) Ownership
of this string resource is *moved* to the variable `s` (of type `String`) when
`String::from` returns on Line 2.

The `println!` macro does not cause a change in ownership (we say more about
`println!` later.)

At the end of the `main` function, the variable `s` goes out of scope. It has
ownership of the string resource, so Rust will *drop*, i.e. deallocate, the
resource at this point. We do not need an explicit `free` or `delete` like we
would in C or C++, nor is there any run-time garbage collection overhead. 

Hover over the lines and arrows in the visualization next to the code example
above to see a description of the events that occur on each line of code.

## Moves

In the example above, we saw that ownership of the heap-allocated string moved
to the caller when `String::from` returned. This is one of several ways in which
ownership of a resource can move. We will now consider each situation in
more detail. 

### Binding
Ownership can be moved when initializing a binding with a variable. 

In the following example, we define a variable `x` that owns a `String`
resource. Then, we define another variable, `y`, initialized with `x`. This
causes ownership of the string resource to be moved from `x` to `y`. Note that
this behavior is different than than the copying behavior for simple types like
integers that we discussed in the previous section. 

<div class="flex-container vis_block" style="position:relative; margin-left:-75px; margin-right:-75px; display: flex;">
  <object type="image/svg+xml" class="string_from_move_print code_panel" data="assets/code_examples/string_from_move_print/vis_code.svg"></object>
  <object type="image/svg+xml" class="string_from_move_print tl_panel" data="assets/code_examples/string_from_move_print/vis_timeline.svg" style="width: auto;" onmouseenter="helpers('string_from_move_print')"></object>
</div>

This code prints `hello`.

At the end of the function, both `x` and `y` go out of scope (their lifetimes
have ended). `x` does not own a resource anymore, so nothing special happens.
`y` does own a resource, so its resource is dropped. Hover over the
visualization to see how this works.

Each resource must have a unique owner, so `x` will no longer own the `String`
resource after it is moved to `y`. This means that access to the resource
through `x` is no longer possible. Think of it like handing a resource to
another person: you no longer have access to it once it has moved. For
example, the following generates a compiler error:

```rust
fn main() {
    let x = String::from("hello");
    let y = x;
    println!("{}", x) // ERROR: x does not own a resource
}
```
The compiler error actually says `borrow of moved value: x` (we will discuss what
*borrow* means in the next section.)

If we move to a variable that has a different scope, e.g. due to curly braces, 
then you can see by
hovering over the visualization that the resource is dropped at the end of `y`'s
scope rather than at the end of `x`'s scope.

```rv
fn main() {
    let x = String::from("hello");
    let z = {
        let y = x;
        println!("{}", y);
        // ...
    };
    println!("Hello, world!");
}
```

This code prints `hello` on one line and `Hello, world!` on the next.

### Assignment

As with binding, ownership can be moved by assignment to a mutable variable,cd
e.g. `y` in the following example.

```rv
fn main() {
  let x = String::from("hello");
  let mut y = String::from("test");
  y = x;
}
```

When `y` acquires ownership over `x`'s resource on Line 4, the resource it
previously acquired (on Line 3) no longer has an owner, so it is dropped.

### Function Call

Ownership is moved into a function when it is called with a resource argument. 
As an example, 
below we see that ownership of the string resource in `main` is moved from `s`
to the `takes_ownership` function. Consequently, when `s` goes out of scope at
the end of `main`, there is no owned string resource to be dropped.

```rv
fn main() {
    let s = String::from("hello");
    takes_ownership(s);
    // println!("{}", s) // won't compile if added
}

fn takes_ownership(some_string: String) {
    println!("{}", some_string);
}
```

This code prints `hello`.

From the perspective of `takes_ownership`, it can be assumed that the argument
variable `some_string` will receive ownership of a `String` resource from the
caller (each time it is called). The argument variable `some_string` goes out of
scope at the end of the function, so the resource that it owns is dropped at
that point.

### Return

Finally, ownership can be returned from a function. 

In the following example, `f` allocates a `String` and returns it to the
caller. Ownership is moved from `x` to the caller, so there is no owned resource
to be dropped at the end of `f`. Instead, the resource is dropped when the new
owner, `s`, goes out of scope at the end of `main`. (If the `String` were
dropped at the end of `f`, there would be a use-after-free bug in `main` on Line
9!)

```rv
fn f() -> String {
    let x = String::from("hello");
    // ...
    x
} 
  
fn main() {
    let s = f();
    println!("{}", s);
}
```

This code prints `hello`.
