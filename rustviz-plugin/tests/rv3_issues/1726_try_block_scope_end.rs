#![feature(try_blocks)]
fn g(o: Option<()>) {
    let _: Option<()> = try {
        let d = String::new();
        o?;
        drop(d);
    };
}
