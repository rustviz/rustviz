fn main() {
    let mut <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="4">String::from</tspan>("hello");
    let <tspan data-hash="3">y</tspan> = <tspan data-hash="1">&amp;mut x</tspan>;
    <tspan class="fn" data-hash="0" hash="7">f</tspan>(<tspan data-hash="1">&amp;x</tspan>); // ERROR: y is still live
    <tspan class="fn" data-hash="0" hash="6">String::push_str</tspan>(<tspan data-hash="3">y</tspan>, ", world");
}
  
fn <tspan class="fn" data-hash="0" hash="7">f</tspan>(<tspan data-hash="1">x</tspan> : &amp;String) {
    <tspan class="fn" data-hash="0" hash="5">println!</tspan>("{}", <tspan data-hash="1">x</tspan>);
}