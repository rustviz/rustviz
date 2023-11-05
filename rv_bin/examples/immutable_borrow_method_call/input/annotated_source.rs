fn main() {
    let <tspan data-hash="1">s</tspan> = <tspan class="fn" data-hash="0" hash="4">String::from</tspan>("hello");
    let <tspan data-hash="2">len1</tspan> = <tspan class="fn" data-hash="0" hash="5">String::len</tspan>(<tspan data-hash="1">&amp;s</tspan>);
    let <tspan data-hash="3">len2</tspan> = <tspan data-hash="1">s</tspan>.<tspan class="fn" data-hash="0" hash="6">len</tspan>(); // shorthand for the above
    <tspan class="fn" data-hash="0" hash="7">println!</tspan>("len1 = {} = len2 = {}", <tspan data-hash="2">len1</tspan>, <tspan data-hash="3">len2</tspan>);
}