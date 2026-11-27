struct P { name: String, tag: String }
fn fields(one: P, two: P) {
    {
        let b = Box::new(one);
        drop(b.name);
    }
    {
        let b = Box::new(two);
        drop(b.tag);
    }
}
