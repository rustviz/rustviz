struct Foo {
    <tspan data-hash="2">x</tspan>: i32,
    <tspan data-hash="3">y</tspan>: String,
}

fn main() {
    let <tspan data-hash="4">_y</tspan> = <tspan class="fn" data-hash="0" hash="5">String :: from</tspan>("bar");
    let <tspan data-hash="1">f</tspan> = Foo { <tspan data-hash="2">x</tspan>: 5, <tspan data-hash="3">y</tspan>: <tspan data-hash="4">_y</tspan> };
    <tspan class="fn" data-hash="0" hash="6">println!</tspan>("{}", f.x);
    <tspan class="fn" data-hash="0" hash="6">println!</tspan>("{}", f.y);
}