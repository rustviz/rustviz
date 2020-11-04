<tspan fill="#AAA">1  </tspan>fn main() {
<tspan fill="#AAA">2  </tspan>    let <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="6">String::from</tspan>("hello");
<tspan fill="#AAA">3  </tspan>    let <tspan data-hash="2">y</tspan> = <tspan data-hash="1">&amp;x</tspan>;
<tspan fill="#AAA">4  </tspan>    let <tspan data-hash="3">z</tspan> = <tspan data-hash="1">&amp;x</tspan>;
<tspan fill="#AAA">5  </tspan>    <tspan class="fn" data-hash="0" hash="7">f</tspan>(<tspan data-hash="2">y</tspan>, <tspan data-hash="3">z</tspan>)
<tspan fill="#AAA">6  </tspan>}
<tspan fill="#AAA">7  </tspan>
<tspan fill="#AAA">8  </tspan>fn <tspan class="fn" data-hash="0" hash="7">f</tspan>(<tspan data-hash="4">s1</tspan> : &amp;String, <tspan data-hash="5">s2</tspan> : &amp;String) {
<tspan fill="#AAA">9  </tspan>    <tspan class="fn" data-hash="0" hash="7">println!</tspan>("{} and {}", <tspan data-hash="4">s1</tspan>, <tspan data-hash="5">s2</tspan>)
<tspan fill="#AAA">10 </tspan>}