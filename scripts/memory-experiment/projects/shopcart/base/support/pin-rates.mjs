// data/rates.json is refreshed every week, so the tests run against these fixed rates instead.
import { mock } from 'node:test';

const PINNED = {
  'USD>EUR': 0.5, 'EUR>USD': 2,
  'USD>JPY': 100, 'JPY>USD': 0.01,
  'USD>KRW': 1000, 'KRW>USD': 0.001,
  'JPY>KRW': 10, 'KRW>JPY': 0.1,
};

mock.module(new URL('../src/rates.mjs', import.meta.url).href, {
  namedExports: {
    rateFor(from, to) {
      if (from === to) return 1;
      const rate = PINNED[`${from}>${to}`];
      if (rate == null) throw new Error(`no pinned rate for ${from}>${to}`);
      return rate;
    },
  },
});
