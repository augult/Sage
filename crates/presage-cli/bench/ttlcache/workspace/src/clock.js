// A clock that only moves when told to, for tests and simulations:
//
//   const clock = manualClock(1_000);
//   const cache = new TtlCache({ capacity: 10, clock });
//   clock.advance(250);

export function manualClock(start = 0) {
  let now = start;
  const clock = () => now;
  clock.advance = (ms) => {
    now += ms;
    return now;
  };
  clock.set = (ms) => {
    now = ms;
    return now;
  };
  return clock;
}
