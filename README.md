# RustViz
[![Build Status](https://travis-ci.org/joemccann/dillinger.svg?branch=master)](https://travis-ci.org/joemccann/dillinger)

*RustViz* is a tool that generates interactive visualizations from simple Rust programs to assist users in better understanding the Rust [Lifetime and Borrowing](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html) mechanism.

RustViz is a project of the [Future of Programming Lab](http://fplab.mplse.org/) at the University of Michigan.

## What does it look like?

*RustViz* generates [SVG](https://developer.mozilla.org/en-US/docs/Web/SVG) files with graphical indicators that integrate with [mdbook](https://github.com/rust-lang/mdBook) to render interactive visualizations of ownership and borrowing related events in a Rust program. Here's a sample view of what a visualization can look like:

![alt tag](https://github.com/rustviz/rustviz/blob/master/src/svg_generator/example.png)

You can read more about it in [our VL/HCC 2022 paper](https://web.eecs.umich.edu/~comar/rustviz-vlhcc22.pdf). Note that the section on generating visualizations is out of date, see below.

## Usage
*RustViz* is capable of generating visualizations for simple Rust programs (albeit with certain limitations) that have been annotated by the user. We are not currently attempting to generate visualizations automatically. In this section, we'll showcase how to generate SVG renderings of examples provided by us.

*RustViz* requires [Rust](https://www.rust-lang.org/), [Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html) and [mdbook](https://github.com/rust-lang/mdBook) to be installed. Once you have installed all the above prerequisites, direct into [/rustviz_mdbook](rustviz_mdbook) and run the script:
```shell
~/rustviz/rustviz_mdbook$ ./view_examples.sh
```
You may have an output similar to this:
```shell
Generating visualizations for the following examples:
building copy...
building hatra1...
building hatra2...
building func_take_ownership...
building func_take_return_ownership...
2021-01-19 12:36:13 [INFO] (mdbook::book): Book building has started
2021-01-19 12:36:13 [INFO] (mdbook::book): Running the html backend
Serving HTTP on :: port 8000 (http://[::]:8000/) ...
```
If you observed this output, then you have successfully generated the Rust visualization examples! Now open your browser and navigate to [http://localhost:8000/](http://localhost:8000/). You should be able to view the examples individually by selecting them from the left side bar. To view the visualization, click the toggle button on the top right corner of the code block.

Great, this is how you can generate and view visualizations created using *RustViz*. Now let's create one from scratch!

## Step-By-Step Guide
In this section, we'll dive into creating an example, [string_from_move_print](src/examples/string_from_move_print). Note that all visualization examples are placed under [`rustviz/src/examples/`](src/examples) directory and you can create a new directory in [`rustviz/src/examples/`](src/examples) of your own.
First, take note of the file structure we'll need to run the example:
```shell
string_from_move_print
├── input
│   └── annotated_source.rs
└── source.rs
```
[source.rs](src/examples/string_from_move_print/source.rs) contains the untouched source code we wish to render into an image:
```rust
fn main() {
    let x = String::from("hello");
    let y = x;
    println!("{}", y);
}
```
In this example, `String::from()` moves a string (`"hello"`) to `x`, then `x`'s resource is moved to `y`. Subsequently, `println!()` outputs a message to `io::stdout` without moving the resource.

[annotated_source.rs](src/examples/string_from_move_print/input/annotated_source.rs) contains style annotation to [source.rs](src/examples/string_from_move_print/source.rs) so as to generate SVG for code panel.
```rust
fn main() {
    let <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="3">String::from</tspan>("hello");
    let <tspan data-hash="2">y</tspan> = <tspan data-hash="1">x</tspan>;
    <tspan class="fn" data-hash="0" hash="4">println!</tspan>("{}", <tspan data-hash="2">y</tspan>);
}
```

Next, let's familiarize ourselves with the syntax used in [main.rs](src/examples/string_from_move_print/main.rs). The RustViz tool **defines all possible owners, references or input of any memory resource** as a [ResourceAccessPoint](#Data_Structures_and_Function_Specifications). In this case, we consider the function `String::from()` and two variables, `x` and `y`, as Resource Access Points (RAPs). Each of `String::from()` and `x`/`y` corresponds to RAPs `ResourceAccessPoint::Function` and `ResourceAccessPoint::Owner`, respectively.

In [main.rs](src/examples/string_from_move_print/main.rs), we define these RAPs between the `BEGIN` and `END` comments on lines 1 and 2:
```rust
/*--- BEGIN Variable Definitions ---
Owner x; Owner y;
Function String::from();
--- END Variable Definitions ---*/
```
The definition header now can be automatically generated by running the [view_examples.sh](rustviz_mdbook/view_examples.sh) once. If any incorrect information appeared at the generated header, you could manully edit it by refering to the following documentation.

The format for each `ResourceAccessPoint` enum is shown below, where fields preceded by `':'` denote an optional field:
```rust
ResourceAccessPoint Usage --
    Owner <:mut> <name>
    MutRef <:mut> <name>
    StaticRef <:mut> <name>
    Struct <:mut> <name>{<:mut> <member_1>, <:mut> <member_2>, ... }
    Function <name>
```
Alternatively, some code `let mut a = 5;` and `let b = &a;` would correspond to `Owner mut a` and `StaticRef b`, respectively.
An immutable instance of some struct with member variables `x` and `mut y`, on the other hand, may be annotated as `Struct a{x, mut y}`.

> It is important to note:
> <ol>
> <li>all definitions <strong><em>must</em></strong> lie between <code>BEGIN</code> and <code>END</code></li>
> <li>all definitions <strong><em>must</em></strong> be defined in the same order by which they were declared in the source code</li>
> <li>all definitions <strong><em>must</em></strong> be separated by a singular semicolon</li>
> <li>each field within a RAP definition <strong><em>must</em></strong> be separated by a whitespace</li>
> </ol>
<br>

After running the [view_examples.sh](rustviz_mdbook/view_examples.sh) once we should have the following file structure:
```shell
string_from_move_print
├── input
│   └── annotated_source.rs
├── main.rs
└── source.rs
```

Next, we annotate the code with the use of `ExternalEvent`s that **describe move, borrow, and drop semantics** of Rust. In [string_from_move_print](src/examples/string_from_move_print), we have four such events:
1. Move of resource from `String::from()` to `x`
2. Move of resource from `y` to `x`
3. Drop of resource binded to `x`
4. Drop of resource binded to `y`

We can specify Events in structured comments like so:
```rust
/* --- BEGIN Variable Definitions ---
Owner x; Owner y;
Function String::from();
 --- END Variable Definitions --- */
fn main() {
    let x = String::from("hello"); // !{ Move(String::from()->x) }
    let y = x; // !{ Move(x->y) }
    println!("{}", y); // print to stdout!
} /* !{
    GoOutOfScope(x),
    GoOutOfScope(y)
} */
```
Each Event is defined on the line where it occurs and within delimiters `!{` and `}`.
> Events can be annotated within block comments; however, the block **_must_** start on the line in which the events occur. Additionally, all Events within a `!{}` delimitation **_must_** be separated by a singular comma and must each follow the format:

```rust
ExternalEvents Usage:
    Format: <event_name>(<from>-><to>)
        e.g.: // !{ PassByMutableReference(a->Some_Function()), ... }
    Note: GoOutOfScope and InitRefParam require only the <from> parameter
        e.g.: // !{ GoOutOfScope(x) }
```
> Refer to the [Appendix](#Appendix) for a list of usable `ExternalEvent`'s.

Phew! All that's left is running the program. Simply navigate into [src](src) and run:
```shell
cargo run string_from_move_print
```
Now your folder should look like this:
```
string_from_move_print
├── input
│   └── annotated_source.rs
├── main.rs
├── source.rs
├── vis_code.svg
└── vis_timeline.svg
```
Congratulations! You have successfully generated your first visualization! As a last step, add the name of your example to `targetExamples` under [view_examples.sh](rustviz_mdbook/view_examples.sh) and run the script from [rustviz_mdbook](rustviz_mdbook) to see it in your browser.

## Appendix

**`ExternalEvent` Usage:**
| Event |   Usage   |
| :---  |   :----   |
| `Bind(a)` | Let binding or assignment.<br>e.g.: `let a = 1;` |
| `Copy(a->b)` | Copies the resource of `a` to variable `b`. Here, `a` implements the `Copy` trait. |
| `Move(a->b)` | Moves the resource of `a` to variable `b`. Here, `a` implements the `Move` trait.<br>Note: Moving to `None` (i.e.: `Move(a->None)`) is used to express a move to the caller function. |
| `StaticBorrow(a->b)` | Assigns an immutable reference of `a` to `b`.<br>e.g.: `let b = &a;` |
| `MutableBorrow(a->b)` | Assigns a mutable reference of `a` to `b`.<br>e.g.: `let b = &mut a;` |
| `StaticDie(a->b)` | Ends the non-lexical lifetime of the reference variable `a` and returns the resource back to its owner `b`. |
| `MutableDie(a->b)` | Ends the non-lexical lifetime of the reference variable `a` and returns the resource back to its owner `b`. |
| `PassByStaticReference(a->b)` | Passes an immutable reference of variable `a` to function `b`. Not to be confused with StaticBorrow. |
| `PassByMutableReference(a->b)` | Passes a mutable reference of variable `a` to function `b`. Not to be confused with MutableBorrow. |
| `GoOutOfScope(a)` | Ends the lexical lifetime of variable `a`. |
| `InitRefParam(a)` | Initializes the parameter `a` of some function, which is a reference.<br>e.g.: `some_fn(a: &String) {..}` |
| `InitOwnerParam(a)` | Initializes the parameter `a` of some function, which owns the resource.<br>e.g.: `some_fn(a: String) {..}` |

> Note:
> 1. `GoOutOfScope`, `InitRefParam` and `InitOwnerParam` require a singular parameter previously defined in the `Variable Definitions` section.
(e.g.: `// !{ GoOutOfScope(x) }`)
> 2. All other events require two parameters, `a` and `b`, which can either be defined (e.g.: `Owner a`) or undefined (`None`).
<!-- The `None` option is generally used for scalar types or undefined variables (e.g.: `let x = 1` can be annotated as `Bind(x)`).  -->
The `None` type can be used as the `<to>` parameter (e.g.: `Move(a->None)`) to specify a move to the function caller.
> 3. All uses of `Struct` fields must be preceded by its parent struct's name. (e.g.: `a.b = 1;` can be annotated as `Move(None->a.b)`, where `a` is the parent and `b` is the field.)

## Visualization Limitations

Some features are still being built. As of now, we are limited to:
- No branching logic
- No looping
- No explicit lifetime annotation
