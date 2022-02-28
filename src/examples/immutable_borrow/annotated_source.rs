fn main() {
    let <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="3">String::from</tspan>("hello");
    <tspan class="fn" data-hash="0" hash="4">f</tspan>(<tspan data-hash="1">&amp;x</tspan>); 
    <tspan class="fn" data-hash="0" hash="5">println!</tspan>("{}", <tspan data-hash="1">x</tspan>);
}

fn <tspan class="fn" data-hash="0" hash="4">f</tspan>(<tspan data-hash="2">s</tspan> : &amp;String) {
    <tspan class="fn" data-hash="0" hash="5">println!</tspan>("{}", <tspan data-hash="2">*s</tspan>);
}