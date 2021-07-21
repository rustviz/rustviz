For this extra credit assignment, you will generate visualizations of Rust programs by annotating code according to the specifications found in the README file.

We have provided the file structure needed to start the assignment, but you will have to edit [main.rs](src/examples/extra_credit/main.rs) in order to generate the corresponding images. You are expected to annotate all move, copy, and borrow events that occur; however, there is no need to do this for the `println!` function.

The final product will be two SVG files, `vis_code.svg` and `vis_timeline.svg`, that together form a visualization of lifetimes and data flow in Rust.