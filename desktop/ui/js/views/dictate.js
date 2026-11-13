// The microphone of a chat bar: what is said goes into the box as text, where the caret is. The
// words come from the webview's own recogniser (the Web Speech API) where it has one; nothing here
// records sound or sends it anywhere itself.

import { icon } from '../dom.js';
import { barButton } from './common.js';

const Recognition = window.SpeechRecognition ?? window.webkitSpeechRecognition;

// Why a recogniser gave up, in a few words. Ending it from here (`aborted`) and silence
// (`no-speech`) are not failures.
const WHY = {
  'not-allowed': 'the microphone is not allowed here',
  'service-not-allowed': 'speech recognition is not allowed here',
  'audio-capture': 'no microphone was found',
  network: 'the speech service could not be reached',
  'language-not-supported': 'the language is not supported',
};
const QUIET = ['aborted', 'no-speech'];

/**
 * `box` is the bar's text box; `fail(text)` shows over the bar what went wrong. Gives
 * `{ button, stop() }`: the button starts the listening and ends it, `stop()` ends it from outside.
 */
export function dictation({ box, fail }) {
  // The recogniser, while it listens.
  let heard = null;
  const button = barButton('dictate', 'Dictate', icon('mic', 16), () => (heard ? heard.stop() : listen()), 'bare');

  function paint() {
    const label = heard ? 'Stop dictating' : 'Dictate';
    button.title = label;
    button.setAttribute('aria-label', label);
    button.setAttribute('aria-pressed', String(Boolean(heard)));
  }

  function listen() {
    if (!Recognition) return fail('Dictation is not available: this window has no speech recognition.');
    const session = new Recognition();
    session.continuous = true;
    session.interimResults = true;
    session.lang = navigator.language;
    // What is said takes the place of the selection, as typing would, set off from the word before it.
    const at = box.selectionStart;
    const lead = at > 0 && !/\s$/.test(box.value.slice(0, at)) ? ' ' : '';
    let length = box.selectionEnd - at;
    session.onresult = (event) => {
      // Everything heard since the start, with the part still being made out.
      const words = [...event.results].map((result) => result[0].transcript).join('').trim();
      const said = words ? lead + words : '';
      box.setRangeText(said, at, at + length, 'end');
      length = said.length;
      box.dispatchEvent(new Event('input', { bubbles: true }));
    };
    session.onerror = (event) => {
      if (!QUIET.includes(event.error)) fail(`Dictation stopped: ${WHY[event.error] ?? event.error}.`);
    };
    session.onend = () => {
      if (heard === session) heard = null;
      paint();
    };
    try {
      session.start();
    } catch (problem) {
      return fail(`Dictation did not start: ${problem?.message ?? problem}`);
    }
    heard = session;
    paint();
    box.focus();
  }

  paint();
  return { button, stop: () => heard?.abort() };
}
