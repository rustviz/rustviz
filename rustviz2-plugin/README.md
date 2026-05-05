# RustViz 2
*RustViz* is a tool that generates interactive visualizations from simple Rust programs to assist users in better understanding the Rust [Lifetime and Borrowing](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html) mechanism.

RustViz is a project of the [Future of Programming Lab](http://fplab.mplse.org/) at the University of Michigan.

## What does it look like?

*RustViz* generates [SVG](https://developer.mozilla.org/en-US/docs/Web/SVG) files with graphical indicators that integrate with [mdbook](https://github.com/rust-lang/mdBook) to render interactive visualizations of ownership and borrowing related events in a Rust program. When rendered in mdbook the diagrams use embedded Javascript to display specific highlighting information. Here's a sample view of what a visualization can look like:

![alt tag](./src/svg_generator/rv2_example.png)

You can read more about the first version of RustViz in [our paper](https://web.eecs.umich.edu/~comar/rustviz-hatra20.pdf).

## Usage
To build from source:
  * Clone the repository
  * Navigate to `rv-plugin/`
  * Install the plugin with: `cargo install --path . --locked` (this will need to be done each time you want to compile any modifications made)
  * Modify (or save) `/test-crate/lib.rs` with your example code (this is important since the compiler won't re-compile code unless a change has been made)
  * Run the plugin by navigating to `/test-crate/` and run `cargo rv-plugin -w > output.txt`
  * The resulting SVG files will be found at `test-crate/vis*`
  * To see the resulting svg files rendered in mdbook use the `test-ex.sh` script
  
To use mdbook:
We provide an [mdbook](https://github.com/rust-lang/mdBook) preprocessor that embeds diagrams into an mdBook. See
[`mdbook-rustviz/`](../mdbook-rustviz/) for instructions.

The hosted playground is at <https://rustviz.github.io/playground/>.
  
## Limitations
RustViz is an educational tool meant to provide insight about Rust to beginners, it is also still actively in development, which means that it only supports a subset of all Rust features. For example, currently we do not support conditional borrowing logic, for-loops, conditional let-bindings and more. We are working to add more of these features, however RustViz is a learning tool for beginners and is not meant to encapsulate all Rust programs.

### Future Goals
We would like to integrate information from the MIR phase of the compiler to allow for 
more complicated borrowing logic as well as simplify some of the work we have to do in the HIR
phase.

To fix/implement:
- [x] Handle owners that are declared inside conditional blocks
- [x] Typecheck function ctxt to determine what type of return annotation to make
- [x] Implement new state calculation system
- [ ] Remove struct members that are not utilized from the timeline
- [ ] Implement hoverable anonymous owner interactions in code panel
- [ ] Weird phantom annotated src bug that seems to appear when there are \t characters
- [ ] Add highlighting for passbyref events
- [ ] Implement For-loops (really just desugared match expr)
- [x] last (black) data-hash doesn't render properly
- [x] Fix Resource dropping (breaks with conditionals it seems)
- [x] Reference aliasing
- [x] Fix annotated source gen to handle `</>` characters 
- [ ] Let-if/match expressions (new conditional move event)
- [x] Conditional lifetime logic and visualization
- [x] Bad stuff happens when you don't put a semi-colon at the end of a void stmt (at the end of a block)
- [ ] Chained method calls (goes hand in hand with anonymous owner interactions) (get(), get_mut())
- [ ] lifetimes that are 'captured' by conditional statements (use MIR)
- [ ] Struct Box kind of buggy
- [ ] JSONify the output
