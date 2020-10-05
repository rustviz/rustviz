# Rust-Resource-Timeline
*rust-resource-timeline* (rrt) is a Rust [Lifetime and Borrowing](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html) visualization library.

## Usage
Currently, this repo is not in any state to be used, but you can read the [library data](src/lib.rs) to get a sense of the data structure that corresponds to the design.

Update: we are starting to support some build. 
Primary examples:

Use: 
* Please use `cargo install --git https://github.com/gab-umich/mdBook.git mdbook` to install a specifically fine tuned version of the mdbook command before proceeding.
* you can 
* `cargo run --example <name_of_example>` to build an SVG related to a certain piece of code.
	* for instance, `cargo run --example book_04_01_02` will trigger the running of [examples/book_04_01_02/main.rs](examples/book_04_01_02/main.rs). This will in-turn call dependencies of its execution: the [main.rs](examples/book_04_01_02/main.rs) will take in the [examples/book_04_01_02/annotated_source.rs](examples/book_04_01_02/annotated_source.rs), and compute the [rendering.svg](examples/book_04_01_02/rendering.svg) by calling functions from [src/lib.rs](src/lib.rs)

## Design Philosophy
I will discuss both current design process and choices in the document here: [docs/design_logic.md](docs/design_logic.md)
