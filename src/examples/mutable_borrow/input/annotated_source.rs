fn main() {
    let mut <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="3">String::from</tspan>("hello");
    <tspan class="fn" data-hash="0" hash="4">world</tspan>(&amp;mut <tspan data-hash="1">x</tspan>);
    <tspan class="fn" data-hash="0" hash="5">println!</tspan>("{}", <tspan data-hash="1">x</tspan>);
}

fn <tspan class="fn" data-hash="0" hash="4">world</tspan>(<tspan data-hash="2">s</tspan> : &amp;mut String) {
    <tspan data-hash="2">s</tspan>.<tspan class="fn" data-hash="0" hash="6">push_str</tspan>(", world");
}
