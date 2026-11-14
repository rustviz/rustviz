// Pre-canned examples for the playground dropdown. The groups follow the
// chapters of the RustViz tutorial (https://rustviz.github.io/tutorial/,
// source: rustviz/tutorial, src/*.md): Motivation, Ownership, Borrowing,
// Conditionals, Loops, Closures, Structs, Threads.
//
// Most examples mirror the tutorial's figures, i.e. the ```rv code blocks
// in those chapters, copied verbatim (including their `// rustviz: hide`
// markers), plus a few extras that are not in the tutorial (marked "Not
// from the tutorial" below). Every example was checked tooltip by
// tooltip against real Rust semantics, and each must compile and render:
// the build pre-renders them all (scripts/prerender.ts) and fails if
// one does not.
//
// To change the set, edit the chapter's group below and rebuild.

export type Example = {
  /** Human-readable label for the dropdown option. */
  name: string;
  /** Rust source as it lands in the editor when selected. */
  code: string;
};

export type ExampleGroup = {
  /** Chapter title from the tutorial; used as <optgroup> label. */
  chapter: string;
  examples: Example[];
};

export const exampleGroups: ExampleGroup[] = [
  {
    chapter: 'Motivation',
    examples: [
      {
        name: "References in action",
        code: `fn main() {
    let mut s = String::from("hello");
    let mut q = String::from("skipped"); // rustviz: skip

    let r1 = &s;
    let r2 = &s;
    assert!(compare_strings(r1, r2));

    let r3 = &mut s;
    clear_string(r3);
}

fn compare_strings(a: &String, b: &String) -> bool { // rustviz: hide
    *a == *b
}

fn clear_string(_s: &mut String) { // rustviz: hide
    // body elided
}`,
      },
    ],
  },
  {
    chapter: 'Ownership',
    examples: [
      {
        name: "Print a String",
        code: `fn main() {
    let s = String::from("hello");
    println!("{}", s);
}`,
      },
      {
        name: "Move and print a String",
        code: `fn main() {
    let x = String::from("hello");
    let y = x;
    println!("{}", y);
}`,
      },
      {
        name: "Move across scope",
        // Note: the source.rs in rustviz-tutorial has `println(...)`
        // (no !) on line 5 — that's a typo in the book that doesn't
        // affect RV1 (annotation-based, doesn't actually compile the
        // snippet). RV2's plugin runs through rustc, so we need the
        // macro form to compile.
        code: `fn main() {
    let x = String::from("hello");
    let z = {
        let y = x;
        println!("{}", y);
        // ...
    };
    println!("Hello, world!");
}`,
      },
      {
        name: "Move on assignment",
        code: `fn main() {
    let x = String::from("hello");
    let mut y = String::from("test");
    y = x;
}`,
      },
      {
        name: "Function takes ownership",
        code: `fn main() {
    let s = String::from("hello");
    takes_ownership(s);
    // println!("{}", s) // won't compile if added
}

fn takes_ownership(some_string: String) {
    println!("{}", some_string);
}`,
      },
      {
        name: "Move on function return",
        // Note: the source.rs in rustviz-tutorial omits the `-> String`
        // return type. RV1's plugin didn't actually compile the snippet
        // so the error was invisible there; RV2 runs through rustc and
        // refuses a `()`-returning fn that yields a String, so we add
        // the return type to make the snippet typecheck.
        code: `fn f() -> String {
    let x = String::from("hello");
    // ...
    x
}

fn main() {
    let s = f();
    println!("{}", s);
}`,
      },
      {
        // Not from the tutorial — added to demo the per-fn
        // timeline layout: each fn has its own `x` with its own
        // column, even though the names collide. The plugin used to
        // share a single column for both (one would silently
        // clobber the other in the global raps map).
        name: "Same name in different functions",
        code: `fn main() {
    let x = String::from("main's hello");
    helper();
    println!("{}", x);
}

fn helper() {
    let x = String::from("helper's hello");
    println!("{}", x);
}`,
      },
    ],
  },
  {
    chapter: 'Borrowing',
    examples: [
      {
        name: "Function takes and returns ownership",
        code: `fn take_and_return_ownership(some_string: String) -> String {
    println!("{}", some_string);
    some_string
}

fn main() {
    let mut s = String::from("hello");
    s = take_and_return_ownership(s);
    println!("{}", s);   // OK
}`,
      },
      {
        name: "Immutable borrow",
        code: `fn main() {
    let x = String::from("hello");
    f(&x);
    println!("{}", x);
}

fn f(s: &String) {
    println!("{}", *s);
}`,
      },
      {
        name: "Immutable borrow (method call)",
        code: `fn main() {
    let s = String::from("hello");
    let len1 = String::len(&s);
    let len2 = s.len(); // shorthand for the above
    println!("len1 = {} = len2 = {}", len1, len2);
}`,
      },
      {
        name: "Multiple immutable borrows",
        code: `fn main() {
    let x = String::from("hello");
    let y = &x;
    let z = &x;
    f(y, z);
}

fn f(s1: &String, s2: &String) {
    println!("{} and {}", s1, s2);
}`,
      },
      {
        name: "Mutable borrow (method call)",
        code: `fn main() {
    let mut s1 = String::from("Hello");
    let s2 = String::from(", world");
    String::push_str(&mut s1, &s2);
    s1.push_str(&s2); // shorthand for the above
    println!("{}", s1); // prints "Hello, world, world"
}`,
      },
      {
        name: "Non-lexical lifetimes",
        code: `fn main() {
    let mut x = String::from("Hello");
    let y = &mut x;
    world(y);
    let z = &mut x; // OK, because y's lifetime has ended (last use was on previous line)
    world(z);
    x.push_str("!!"); // Also OK, because y and z's lifetimes have ended
    println!("{}", x);
}

fn world(s: &mut String) {
    s.push_str(", world");
}`,
      },
      {
        // Not from the tutorial. `longest` returns one of its
        // two reference arguments; the signature ties the result's
        // lifetime to both, so both lenders stay borrowed while `r` lives.
        name: "Function returning a reference",
        code: `fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() >= b.len() { a } else { b }
}

fn main() {
    let s1 = String::from("hello");
    let s2 = String::from("hi");
    let r = longest(&s1, &s2);
    println!("{}", r);
}`,
      },
      {
        name: "Reborrowing: passing a &mut twice",
        code: `fn main() {
    let mut s = String::from("hello");
    let r = &mut s;
    add_world(r);
    add_world(r);
    println!("{}", s);
}

fn add_world(t: &mut String) {
    t.push_str(", world");
}`,
      },
      {
        name: "Reborrowing: explicit &mut *r and &*r",
        code: `fn main() {
    let mut s = String::from("hello");
    let r = &mut s;
    let r2 = &mut *r;
    r2.push_str("!");
    let r3 = &*r;
    println!("{}", r3);
    r.push_str("?");
    println!("{}", s);
}`,
      },
    ],
  },
  {
    chapter: 'Conditionals',
    // Branches written on their own lines on purpose. The
    // timeline positions per-branch dots by source line, so the
    // single-line form `if … { … } else { … }` collapses both
    // branches onto one row and renders as a bowtie / X-shape
    // rather than the intended pair. The canonical examples stay
    // multi-line until the renderer grows synthetic offsets for
    // same-line branches.
    //
    // Helper bindings (`cond`, `c1`…) in the examples that are not
    // tutorial figures carry `// rustviz: skip`, and helper fns carry
    // `// rustviz: hide`, so they don't add scaffolding columns to the
    // visualization — the diagrams stay focused on the variable whose
    // ownership / borrow flow the example illustrates.
    examples: [
      {
        name: "Moving in one branch",
        code: `fn main() {
    let s = String::from("hello");
    let keep = false;
    if keep {
        println!("{}", s);
    } else {
        takes_ownership(s);
    }
}

fn takes_ownership(some_string: String) {
    println!("{}", some_string);
}`,
      },
      {
        name: "if/else: move on both sides",
        code: `fn consume(_s: String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let cond = true; // rustviz: skip
    if cond {
        consume(s);
    } else {
        consume(s);
    }
}`,
      },
      {
        name: "Move, then reassign, in both branches",
        code: `fn consume(_s: String) {} // rustviz: hide
fn show(_s: &String) {} // rustviz: hide

fn main() {
    let mut s = String::from("orig");
    let cond = true; // rustviz: skip
    if cond {
        consume(s);
        s = String::from("new");
    } else {
        consume(s);
        s = String::from("alt");
    }
    show(&s);
}`,
      },
      {
        name: "nested if (move propagates outward)",
        code: `fn consume(_s: String) {} // rustviz: hide
fn show(_s: &String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let c1 = true; // rustviz: skip
    let c2 = false; // rustviz: skip
    if c1 {
        if c2 {
            consume(s);
        } else {
            show(&s);
        }
    } else {
        consume(s);
    }
}`,
      },
      {
        name: "match: 3 arms (consume vs borrow)",
        code: `fn consume(_s: String) {} // rustviz: hide
fn show(_s: &String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let n = 1; // rustviz: skip
    match n {
        0 => consume(s),
        1 => show(&s),
        _ => show(&s),
    }
}`,
      },
      {
        name: "Matching on a reference",
        code: `fn main() {
    let name = Some(String::from("Ferris"));
    match &name {
        Some(n) => println!("hello, {}", n),
        None => println!("no name"),
    }
    println!("{:?}", name);
}`,
      },
      {
        name: "Matching by value",
        code: `fn main() {
    let name = Some(String::from("Ferris"));
    match name {
        Some(n) => {
            println!("hello, {}", n);
        }
        None => println!("no name"),
    }
}`,
      },
    ],
  },
  {
    chapter: 'Loops',
    examples: [
      {
        name: "for over a borrowed Vec",
        code: `fn show(s: &String) { // rustviz: hide
    println!("{}", s);
}

fn main() {
    let words = vec![String::from("hi"), String::from("ok")];
    for w in &words {
        show(w);
    }
}`,
      },
      {
        name: "for that moves each element",
        code: `fn consume(_s: String) {} // rustviz: hide

fn main() {
    let words = vec![String::from("hi"), String::from("ok")];
    for w in words {
        consume(w);
    }
}`,
      },
      {
        name: "while let popping a stack",
        code: `fn consume(_s: String) {} // rustviz: hide

fn main() {
    let mut stack = vec![String::from("a"), String::from("b")];
    while let Some(s) = stack.pop() {
        consume(s);
    }
}`,
      },
      {
        // Not from the tutorial. Each iteration creates a
        // String and moves it into the Vec, which owns every one.
        name: "for loop pushing into a Vec",
        code: `fn main() {
    let mut v = Vec::new();
    for _ in 0..3 {
        let s = String::from("item");
        v.push(s);
    }
    println!("{}", v.len());
}`,
      },
      {
        name: "Dropping in each iteration",
        code: `fn main() {
    for i in 0..3 {
        let s = String::from("hello");
        println!("{} {}", i, s);
    }
}`,
      },
      {
        name: "`loop` and `break`",
        code: `fn main() {
    let mut s = String::new();
    let t = loop {
        s.push_str("ab");
        if s.len() >= 6 {
            break s;
        }
    };
    println!("{}", t);
}`,
      },
    ],
  },
  {
    chapter: 'Closures',
    examples: [
      {
        name: "Move closure (single capture)",
        code: `fn main() {
    let s = String::from("hello");
    let show = move || println!("{}", s);
    show();
    show();
}`,
      },
      {
        name: "Move closure (multiple captures)",
        code: `fn main() {
    let s = String::from("hi");
    let t = String::from("bye");
    let f = move || println!("{} {}", s, t);
    f();
}`,
      },
      {
        name: "Borrow closure (immutable)",
        code: `fn main() {
    let s = String::from("hello");
    let show = || println!("{}", s);
    show();
    println!("{}", s);
    show();
}`,
      },
      {
        name: "Borrow closure (mutable)",
        code: `fn main() {
    let mut s = String::from("hello");
    let mut exclaim = || s.push_str("!");
    exclaim();
    exclaim();
    println!("{}", s);
}`,
      },
      {
        name: "Closure that can only be called once",
        code: `fn main() {
    let s = String::from("hello");
    let give_back = || s;
    let t = give_back();
    println!("{}", t);
}`,
      },
    ],
  },
  {
    chapter: 'Structs',
    examples: [
      {
        name: "Struct: Rectangle",
        code: `struct Rect {
    w: u32,
    h: u32,
}

fn main() {
    let r = Rect {
        w: 30,
        h: 50,
    };

    println!(
        "The area of the rectangle is {} square pixels.",
        area(&r)
    );

    println!("The height of that is {}.", r.h);
}

fn area(rect: &Rect) -> u32 {
    rect.w * rect.h
}`,
      },
      {
        name: "Struct: Rectangle with a method",
        code: `struct Rectangle {
    width: u32,
    height: u32,
}

impl Rectangle {
    fn area(&self) -> u32 {
        self.width * self.height
    }
}

fn print_area(rect: &Rectangle) {
    println!(
        "The area of the rectangle is {} square pixels.",
        rect.area() // dot even though it's actually a reference
    );
}

fn main() {
    let r = Rectangle {
        width: 30,
        height: 50,
    };

    print_area(&r);
}`,
      },
      {
        name: "Struct with String",
        code: `struct Foo {
    x: i32,
    y: String,
}

fn main() {
    let _y = String::from("bar");
    let f = Foo { x: 5, y: _y };
    println!("{}", f.x);
    println!("{}", f.y);
}`,
      },
      {
        // Not from the tutorial. A partial move: `p.name` is
        // moved out, but the Copy field `p.age` can still be used.
        name: "Struct field moved out",
        code: `struct Person {
    name: String,
    age: u32,
}

fn main() {
    let p = Person { name: String::from("Ada"), age: 36 };
    let n = p.name;
    println!("{} {}", n, p.age);
}`,
      },
      {
        name: "Struct with lifetime",
        code: `struct Excerpt<'a> {
    p: &'a str,
}

fn some_function() {
    let n = String::from("Ok. I'm fine.");
    let first = n.split('.').next().expect("Could not find a '.'");
    let i = Excerpt {
        p: first,
    };
    // 'i' cannot be returned
    // because the struct outlives 'n'
}

fn main() {
    some_function();
}`,
      },
    ],
  },
  {
    chapter: 'Threads',
    // Not from the tutorial. The spawned closure's body isn't
    // traced (see the closure limitations), but the move of `s` into it
    // and the move of `handle` into `join` are drawn.
    examples: [
      {
        name: "thread::spawn with a move closure",
        code: `use std::thread;

fn main() {
    let s = String::from("hello");
    let handle = thread::spawn(move || {
        println!("{}", s);
    });
    handle.join().unwrap();
}`,
      },
    ],
  },
];
