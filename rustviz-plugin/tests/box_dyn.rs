// `Box<dyn Display>` trait object — issue #76 case 3.
// Same opaque-owner treatment as the sized-`Box` case: one column
// per binding, no `Unique`/`NonNull`/`PhantomData` bleed-through.

use std::fmt::Display;

fn main() {
    let b: Box<dyn Display> = Box::new(String::from("hi"));
    println!("{}", b);
}
