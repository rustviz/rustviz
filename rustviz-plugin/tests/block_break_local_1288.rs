// #1288: a `break 'blk` that carries a local declared inside the labeled
// block (or a field of one) out of it. The block can also end another way
// (its last expression), which drops the local where the block ends; the
// `break` moves it to the receiver instead (and drops the rest of a struct
// it took a field of as it leaves the block). main said "k goes out of
// scope. Its resource is dropped." unconditionally, a drop flag for
// g.name / t.0 ("may have been moved ... can't be used after the block",
// though g is gone after the block anyway), nothing at all for a block
// that is the function's value, and went `State::Invalid` when the way to
// the block's end moves the local.
struct Person { name: String, bio: String }
fn whole(c: bool) -> String {
    let r = 'blk: {
        let k = String::from("a");
        if c { break 'blk k; }
        String::new()
    };
    r
}
fn field(c: bool) -> String {
    let r = 'blk: {
        let g = Person { name: String::from("a"), bio: String::from("b") };
        if c { break 'blk g.name; }
        String::new()
    };
    r
}
fn tuple_field(c: bool) -> String {
    let r = 'blk: {
        let t = (String::from("a"), String::from("b"));
        if c { break 'blk t.0; }
        String::new()
    };
    r
}
fn in_arm(n: i32) -> String {
    let r = 'blk: {
        let a = String::from("a");
        match n {
            1 => break 'blk a,
            _ => {}
        }
        String::new()
    };
    r
}
fn returned(c: bool) -> String {
    'blk: {
        let v = String::from("a");
        if c { break 'blk v; }
        String::new()
    }
}
fn moved_on_way(c: bool) -> String {
    let r = 'blk: {
        let m = String::from("a");
        if c { break 'blk m; }
        drop(m);
        String::new()
    };
    r
}
fn tail_is_local(c: bool) -> String {
    let r = 'blk: {
        let x = String::from("a");
        let y = String::from("b");
        if c { break 'blk x; }
        y
    };
    r
}
fn main() {
    whole(true);
    field(false);
    tuple_field(true);
    in_arm(1);
    returned(false);
    moved_on_way(true);
    tail_is_local(false);
}
