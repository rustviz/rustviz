fn main() {
    let <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>("hello");
    <tspan class="fn" data-hash="0" hash="6">f</tspan>(<tspan data-hash="1">&amp;x</tspan>); 
    <tspan class="fn" data-hash="0" hash="8">println!</tspan>("{}", <tspan data-hash="1">x</tspan>)
}

fn <tspan class="fn" data-hash="0" hash="6">f</tspan>(<tspan data-hash="3">s</tspan> : &amp;String) {
    <tspan class="fn" data-hash="0" hash="8">println!</tspan>("{}", <tspan data-hash="3">s</tspan>)
}