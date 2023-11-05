fn main() {
    let mut <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>("Hello");
    let <tspan data-hash="2">y</tspan> = &amp;mut <tspan data-hash="1">x</tspan>;
    <tspan class="fn" data-hash="0" hash="6">world</tspan>(<tspan data-hash="2">y</tspan>);
    let <tspan data-hash="3">z</tspan> = &amp;mut <tspan data-hash="1">x</tspan>; // OK, because y's lifetime has ended (last use was on previous line)
    <tspan class="fn" data-hash="0" hash="6">world</tspan>(<tspan data-hash="3">z</tspan>);
    <tspan data-hash="1">x</tspan>.<tspan class="fn" data-hash="0" hash="7">push_str</tspan>("!!"); // Also OK, because y and z's lifetimes have ended
    <tspan class="fn" data-hash="0" hash="8">println!</tspan>("{}", <tspan data-hash="1">x</tspan>);
}

fn <tspan class="fn" data-hash="0" hash="6">world</tspan>(<tspan data-hash="4">s</tspan> : &amp;mut String) {
    <tspan data-hash="4">s</tspan>.<tspan class="fn" data-hash="0" hash="7">push_str</tspan>(", world");
}