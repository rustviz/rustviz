<tspan fill="#AAA">1  </tspan>fn main() {
<tspan fill="#AAA">2  </tspan>    let mut <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>("hello");
<tspan fill="#AAA">3  </tspan>    <tspan class="fn" data-hash="0" hash="6">world</tspan>(&amp;mut <tspan data-hash="1">&amp;x</tspan>);
<tspan fill="#AAA">4  </tspan>    <tspan class="fn" data-hash="0" hash="7">println!</tspan>("{}", <tspan data-hash="1">x</tspan>)
<tspan fill="#AAA">5  </tspan>}
<tspan fill="#AAA">6  </tspan>
<tspan fill="#AAA">7  </tspan>fn world(<tspan data-hash="2">s</tspan> : &amp;mut String) {
<tspan fill="#AAA">8  </tspan>    <tspan data-hash="2">s</tspan>.<tspan class="fn" data-hash="0" hash="8">push_str</tspan>(", world")
<tspan fill="#AAA">9  </tspan>}