<tspan fill="#AAA">1  </tspan>fn <tspan class="fn" data-hash="0" hash="6">take_and_return_ownership</tspan>(<tspan data-hash="2">some_string</tspan> : String) -> String {
<tspan fill="#AAA">2  </tspan>    <tspan class="fn" data-hash="0" hash="7">println!</tspan>("{}", <tspan data-hash="2">some_string</tspan>);
<tspan fill="#AAA">3  </tspan>    <tspan data-hash="2">some_string</tspan>
<tspan fill="#AAA">4  </tspan>}
<tspan fill="#AAA">5  </tspan>  
<tspan fill="#AAA">6  </tspan>fn main() {
<tspan fill="#AAA">7  </tspan>    let mut <tspan data-hash="1">s</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>("hello");
<tspan fill="#AAA">8  </tspan>    <tspan data-hash="1">s</tspan> = <tspan class="fn" data-hash="0" hash="6">take_and_return_ownership</tspan>(<tspan data-hash="1">s</tspan>);
<tspan fill="#AAA">9  </tspan>    <tspan class="fn" data-hash="0" hash="7">println!</tspan>("{}", <tspan data-hash="1">s</tspan>);   // OK
<tspan fill="#AAA">10 </tspan>}