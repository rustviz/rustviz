// Pre-canned examples for the playground dropdown. Curated set: only
// includes example snippets that the rustviz-tutorial mdBook actually
// renders as RustViz visualizations — i.e., chapter prose embeds them
// via the visualization SVGs (`code_examples/<name>/vis_code.svg` and
// `vis_timeline.svg`). Snippets that the book embeds only as plain
// code blocks (the entire Rust basics chapter — copy, function,
// immutable_variable, mutable_variables, printing) are not part of
// the visual tutorial experience, so they're left out of the
// playground's dropdown.
//
// Snippets in src/assets/code_examples/ that aren't referenced by
// any chapter markdown at all (drafts, abandoned variants — hatra1,
// hatra1_test, mutable_borrow, string_from, extra_credit) are also
// intentionally left out.
//
// Source: rustviz/rustviz-tutorial @ master.
//   src/assets/code_examples/<name>/source.rs is the literal Rust
//   snippet; we vendor it verbatim. The tutorial itself is RV1-era
//   (uses the SVG-template approach), but its source.rs files are
//   plain Rust with no RV1 annotations, so they drop directly into
//   the RV2 playground editor.
//
// A handful of these will fail to visualize today because the RV2
// plugin doesn't yet support every feature the tutorial covers
// (lifetimes, struct-member borrows, chained method calls, etc.).
// That surfaces the current plugin gaps to curious users — see
// rustviz2-plugin/README.md for the up-to-date support matrix.
//
// To refresh: re-run the one-shot fetch pipeline used to seed this
// file (gh api repos/rustviz/rustviz-tutorial/contents/src/assets/
// code_examples/<name>/source.rs --jq '.content' | base64 -d) for
// each name in the list below, then rebuild.

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
        name: "Hands-on tutorial",
        code: `fn main(){
    let mut s = String::from("hello");

    let r1 = &s;
    let r2 = &s;
    assert!(compare_strings(r1, r2));

    let r3 = &mut s;
    clear_string(r3);
}

fn compare_strings(_a: &String, _b: &String) -> bool {
    // body elided
    true
}

fn clear_string(_s: &mut String) {
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
        // Not from the upstream tutorial — added to demo the per-fn
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
        code: `fn take_and_return_ownership(some_string : String) -> String {
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

fn f(s : &String) {
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

fn f(s1 : &String, s2 : &String) {
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

fn world(s : &mut String) {
    s.push_str(", world");
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
        name: "Struct: Rectangle (variant)",
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
    let _y = String :: from("bar");
    let f = Foo { x: 5, y: _y };
    println!("{}", f.x);
    println!("{}", f.y);
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
];
