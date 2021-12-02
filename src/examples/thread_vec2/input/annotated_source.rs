use std::thread;
fn <tspan class="fn" data-hash="0" hash="2">parent</tspan>() {
    static v : [ i32 ; 3] = [1, 2, 3]; // fixed-length array
    let <tspan data-hash="4">handle</tspan> = <tspan class="fn" data-hash="0" hash="3">thread::spawn</tspan>(|| {
        println!("{}", v[0]); // OK, v guaranteed to outlive thread
    });
    <tspan data-hash="4">handle</tspan>.<tspan class="fn" data-hash="0" hash="1">join()</tspan>.<tspan class="fn" data-hash="0" hash="5">unwrap()</tspan>;
}

fn main() {
    <tspan class="fn" data-hash="0" hash="2">parent</tspan>();
}
