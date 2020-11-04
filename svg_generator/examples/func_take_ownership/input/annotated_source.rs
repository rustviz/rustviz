<tspan fill="#AAA">1  </tspan>fn main() {
<tspan fill="#AAA">2  </tspan>    let <tspan data-hash="1">s</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>("hello");
<tspan fill="#AAA">3  </tspan>    <tspan class="fn" data-hash="0" hash="6">takes_ownership</tspan>(<tspan data-hash="1">s</tspan>);
<tspan fill="#AAA">4  </tspan>    // println!("{}", s) // won't compile if added
<tspan fill="#AAA">5  </tspan>}
<tspan fill="#AAA">6  </tspan>
<tspan fill="#AAA">7  </tspan>fn <tspan class="fn" data-hash="0" hash="6">takes_ownership</tspan>(<tspan data-hash="2">some_string</tspan>: String) {
<tspan fill="#AAA">8  </tspan>    <tspan class="fn" data-hash="0" hash="8">println!</tspan>("{}", <tspan data-hash="2">some_string</tspan>)
<tspan fill="#AAA">9  </tspan>}

