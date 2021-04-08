fn main() {
    let <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="6">String::from</tspan>("hello");
    let <tspan data-hash="2">y</tspan> = <tspan data-hash="1">&amp;x</tspan>;
    let <tspan data-hash="3">z</tspan> = <tspan data-hash="1">&amp;x</tspan>;
    <tspan class="fn" data-hash="0" hash="7">f</tspan>(<tspan data-hash="2">y</tspan>, <tspan data-hash="3">z</tspan>);
}

fn <tspan class="fn" data-hash="0" hash="7">f</tspan>(<tspan data-hash="4">s1</tspan> : &amp;String, <tspan data-hash="5">s2</tspan> : &amp;String) {
    <tspan class="fn" data-hash="0" hash="8">println!</tspan>("{} and {}", <tspan data-hash="4">s1</tspan>, <tspan data-hash="5">s2</tspan>);
}