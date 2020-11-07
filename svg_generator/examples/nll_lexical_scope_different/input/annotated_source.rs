<tspan fill="#AAA">1  </tspan>fn main() {
<tspan fill="#AAA">2  </tspan>    let mut <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>("Hello");
<tspan fill="#AAA">3  </tspan>    let <tspan data-hash="2">y</tspan> = &amp;mut <tspan data-hash="1">x</tspan>;
<tspan fill="#AAA">4  </tspan>    <tspan class="fn" data-hash="0" hash="6">world</tspan>(<tspan data-hash="2">y</tspan>);
<tspan fill="#AAA">5  </tspan>    let <tspan data-hash="3">z</tspan> = &amp;mut <tspan data-hash="1">x</tspan>; // OK, because y's lifetime has ended (last use was on previous line)
<tspan fill="#AAA">6  </tspan>    <tspan class="fn" data-hash="0" hash="6">world</tspan>(<tspan data-hash="3">z</tspan>);
<tspan fill="#AAA">7  </tspan>    <tspan data-hash="1">x</tspan>.<tspan class="fn" data-hash="0" hash="7">push_str</tspan>("!!"); // Also OK, because y and z's lifetimes have ended
<tspan fill="#AAA">8  </tspan>    <tspan class="fn" data-hash="0" hash="8">println!</tspan>("{}", <tspan data-hash="1">x</tspan>)
<tspan fill="#AAA">9  </tspan>}
<tspan fill="#AAA">10 </tspan>
<tspan fill="#AAA">11 </tspan>fn <tspan class="fn" data-hash="0" hash="6">world</tspan>(<tspan data-hash="4">s</tspan> : &amp;mut String) {
<tspan fill="#AAA">12 </tspan>    <tspan data-hash="4">s</tspan>.<tspan class="fn" data-hash="0" hash="7">push_str</tspan>(", world")
<tspan fill="#AAA">13 </tspan>}