fn main() {
    let mut <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="3">String::from</tspan>("hello");
    let mut <tspan data-hash="2">y</tspan> = <tspan data-hash="1">&amp;mut x</tspan>;
    <tspan class="fn" data-hash="0" hash="5">f</tspan>(<tspan data-hash="1">&amp;x</tspan>);
    <tspan class="fn" data-hash="0" hash="4">String::push_str</tspan>(<tspan data-hash="2">y</tspan>,<tspan class="fn" data-hash="0" hash="3">String::from</tspan>(", world"));
}


fn <tspan class="fn" data-hash="0" hash="5">f</tspan>(<tspan data-hash="1">x</tspan> : &amp;String) { 
    <tspan class="fn" data-hash="0" hash="7">println!</tspan>("{}",<tspan data-hash="1">x</tspan>);
}