# RustViz
[![Build Status](https://travis-ci.org/joemccann/dillinger.svg?branch=master)](https://travis-ci.org/joemccann/dillinger)

*RustViz* is a tool that generates visualizations from simple Rust programs to assist users in better understanding the Rust [Lifetime and Borrowing](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html) mechanism.

## What does it look like?

*RustViz* generates [SVG](https://developer.mozilla.org/en-US/docs/Web/SVG) files with graphical indicators that integrate with [mdbook](https://github.com/rust-lang/mdBook) to render visualizations of data-flow in Rust programs. Here's a sample view of what a visualization can look like:

![alt tag](https://github.com/rustviz/rustviz/blob/master/example.png)

## Usage
*RustViz* is capable of visualizing simple Rust programs (albeit with certain limitations) via user definition. In this section, we'll showcase how to generate SVG renderings of examples provided by us.

*RustViz* requires [Rust](https://www.rust-lang.org/), [Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html) and [mdbook](https://github.com/rust-lang/mdBook) to be installed. Once you have installed all the above prerequisites, direct into [/test_examples](../test_examples) and run the script:
```shell
~/rustviz/test_examples$ ./test_dsl.sh
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
In this section, we'll dive into creating an example, [string_from_move_print](../svg_generator/examples/string_from_move_print). First, take note of the file structure we'll need to run the example:
```shell
string_from_move_print
├── input
│   └── annotated_source.rs
├── main.rs
└── source.rs
```
[source.rs](../svg_generator/examples/string_from_move_print/source.rs) contains the untouched source code we wish to render into an image:
```rust
fn main() {
    let x = String::from("hello");
    let y = x;
    println!("{}", y);
}
```
In this example, `String::from()` moves a string (`"hello"`) to `x`, then `x`'s resource is moved to `y`. Subsequently, `println!()` outputs a message to `io::stdout` without moving the resource.

Next, let's familiarize ourselves with the syntax used in [main.rs](../svg_generator/examples/string_from_move_print/main.rs). The RustViz tool **defines all possible owners, references or input of any memory resource** as a [ResourceAccessPoint](#Data_Structures_and_Function_Specifications). In this case, we consider the function `String::from()` and two variables, `x` and `y`, as Resource Access Points (RAPs). Each of `String::from()` and `x`/`y` corresponds to RAPs `ResourceAccessPoint::Function` and `ResourceAccessPoint::Owner`, respectively.

In [main.rs](../svg_generator/examples/string_from_move_print/main.rs), we define these RAPs between the `BEGIN` and `END` comments on lines 1 and 2:
```rust
/* --- BEGIN Variable Definitions ---
Owner x, Owner y,
Function String::from()
 --- END Variable Definitions --- */
```
The format for each `ResourceAccessPoint` enum is shown below, where fields preceded by `':'` denote an optional field:
```rust
ResourceAccessPoint Usage --
    Owner <:mut> <name>
    MutRef <:mut> <name>
    StaticRef <:mut> <name>
    Function <name>
```
As another example, some code `let mut a = 5;` and `let b = &a;` would correspond to `Owner mut a` and `StaticRef b`, respectively.

> It is important to note all definitions **_must_** lie between `BEGIN` and `END`, **_must_** be separated by a singular comma, and each field within a RAP definition **_must_** be separated by a whitespace.

Next, we annotate the code with the use of `ExternalEvent`s that **describe move, borrow, and drop semantics** of Rust. In [string_from_move_print](../svg_generator/examples/string_from_move_print), we have four such events:
1. Move of resource from `String::from()` to `x`
2. Move of resource from `y` to `x`
3. Drop of resource binded to `x`
4. Drop of resource binded to `y`

We can use specify Events in structured comments like so:
```rust
/* --- BEGIN Variable Definitions ---
Owner x, Owner y,
Function String::from()
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
    Note: GoOutOfScope and InitializeParam require only the <from> parameter
        e.g.: // !{ GoOutOfScope(x) }
```
> Refer to the [Appendix](#Appendix) for a list of usable `ExternalEvent`'s.

Phew! All that's left is running the program. Navigate to the [dsl](/dsl) directory and run:
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
Congratulations! You have successfully generated your first visualization! As a last step, add the name of your example to `targetExamples` under [test_dsl.sh](../test_examples/test_dsl.sh) and run the script to see it in your browser.

## Appendix

**`ExternalEvent` Usage:**
- `Bind(a->b)` // let binding, e.g.: let x = 1
- `Copy(a->b)`
- `Move(a->b)`
- `StaticBorrow(a->b)`
- `MutableBorrow(a->b)`
- `StaticReturn(a->b)`
- `MutableReturn(a->b)`
- `PassByStaticReference(a->b)`
    - used for Functions, not to be confused with StaticBorrow
- `PassByMutableReference(a->b)`
    - used for Functions, not to be confused with MutableBorrow
- `GoOutOfScope(a)`
- `InitializeParam(a)`
    - only use this event to initialize `function parameters

> Note:
> 1. `GoOutOfScope` and `InitializeParam` require a singular parameter previously defined in the `Variable Definitions` section.
(e.g.: `// !{ GoOutOfScope(x) }`)
> 2. All other events require two parameters, `a` and `b`, which can either be defined (e.g.: `Owner a`) or undefined (`None`).
The `None` option is generally used for scalar types or undefined variables (e.g.: `let x = 1` can be annotated as `Bind(None->x)`).

## Visualization Limitations
Yet to be finished....
