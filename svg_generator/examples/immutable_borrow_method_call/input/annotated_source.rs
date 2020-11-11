<tspan fill="#AAA">1  </tspan>fn main() {
<tspan fill="#AAA">2  </tspan>    let <tspan data-hash="1">s</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>("hello");
<tspan fill="#AAA">3  </tspan>    let <tspan data-hash="2">len1</tspan> = <tspan class="fn" data-hash="0" hash="6">String::len</tspan>(<tspan data-hash="1">&amp;s</tspan>);
<tspan fill="#AAA">4  </tspan>    let <tspan data-hash="3">len2</tspan> = <tspan data-hash="1">s</tspan>.<tspan class="fn" data-hash="0" hash="7">len</tspan>(); // shorthand for the above
<tspan fill="#AAA">5  </tspan>    <tspan class="fn" data-hash="0" hash="8">println!</tspan>("len1 = {} = len2 = {}", <tspan data-hash="2">len1</tspan>, <tspan data-hash="3">len2</tspan>)
<tspan fill="#AAA">6  </tspan>}