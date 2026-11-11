//! Feeds the command sequences the experiment projects produce to the real `LessonBook` and prints
//! what it learns and what it would hint in a later session. No model and no network.

use presage_core::lessons::LessonBook;

enum Step {
    Fail(&'static str, &'static str),
    Ok(&'static str),
    Edit,
}
use Step::*;

const NO_MODULE: &str = "ModuleNotFoundError: No module named 'ledger'\n\nRan 4 tests in 0.001s\n\nFAILED (errors=4)";
const TEST_FAILS: &str = "AssertionError: 'Bread' != 'Bread, milk'\n\nRan 14 tests in 0.002s\n\nFAILED (failures=1)";
const TEST_ERRORS: &str = "TypeError: unhashable type: 'list'\n\nRan 14 tests in 0.002s\n\nFAILED (errors=1)";
const NO_BUILD: &str = "    bin/logkit: 5: .: cannot open /tmp/w/logkit/build/logkit.sh: No such file\n0 passed, 4 failed";
const CASE_FAILS: &str = "FAIL stats: logkit stats\n3 passed, 1 failed";
const NO_SCRIPT: &str = "npm error Missing script: \"test\"\nnpm error A complete log of this run can be found in: /root/.npm/_logs/x.log";
const NO_MOCK: &str = "✖ test/tax.test.mjs (41.3ms)\n  TypeError [Error]: mock.module is not a function\n      at file:///tmp/w/shopcart/support/pin-rates.mjs:11:6\nℹ tests 4\nℹ pass 0\nℹ fail 4";

fn learn(name: &str, steps: &[Step]) -> LessonBook {
    let mut book = LessonBook::in_memory();
    let mut learned = Vec::new();
    for step in steps {
        match step {
            Fail(cmd, out) => assert!(book.on_shell(cmd, false, out).is_none()),
            Ok(cmd) => learned.extend(book.on_shell(cmd, true, "ok").map(|l| l.describe())),
            Edit => book.on_edit(),
        }
    }
    println!("{name}\n    learned: {}", if learned.is_empty() { "nothing".into() } else { learned.join(" | ") });
    book
}

fn hint(book: &LessonBook, cmd: &str, out: &str, what: &str) {
    println!("    later `{cmd}` fails ({what}): {}", book.hint(cmd, out).unwrap_or_else(|| "no hint".into()));
}

fn main() {
    let py = "python3 -m unittest";
    let py_fix = "PYTHONPATH=src python3 -m unittest";
    let book = learn("ledger, tests pass once the path is set", &[Fail(py, NO_MODULE), Ok(py_fix)]);
    hint(&book, py, NO_MODULE, "same friction");
    hint(&book, "python3 -m unittest -v", NO_MODULE, "same friction");
    hint(&book, "python3 -m unittest tests.test_money", NO_MODULE, "same friction");
    hint(&book, "python3 -m pytest", "/usr/bin/python3: No module named pytest", "other cause");
    hint(&book, py_fix, TEST_ERRORS, "a real bug that raises, run with the fix already applied");
    hint(&book, py_fix, TEST_FAILS, "a real assertion failure, run with the fix already applied");
    learn("ledger, a test fails for a real bug until the code is edited", &[Fail(py, NO_MODULE), Fail(py_fix, TEST_FAILS), Edit, Ok(py_fix)]);
    learn("ledger, workaround spelled with discover", &[Fail(py, NO_MODULE), Ok("PYTHONPATH=src python3 -m unittest discover -s tests")]);
    learn("ledger, workaround by changing directory", &[Fail(py, NO_MODULE), Ok("cd src && python3 -m unittest discover -s ../tests")]);
    learn("ledger, real failure then a fix made without the edit tool", &[Fail(py_fix, TEST_FAILS), Ok("sed -i s/a/b/ src/ledger/entries.py"), Ok("PYTHONPATH=src python3 -m unittest -v")]);

    let run = "sh tests/run.sh";
    let book = learn("logkit, tests pass once generated", &[Fail(run, NO_BUILD), Ok("make gen"), Ok(run)]);
    hint(&book, run, NO_BUILD, "same friction");
    hint(&book, run, CASE_FAILS, "a real failing case, build present");
    learn("logkit, generated and run in one command", &[Fail(run, NO_BUILD), Ok("make gen && sh tests/run.sh")]);
    learn("logkit, a case fails for a real bug until lib/ is edited", &[Fail(run, NO_BUILD), Ok("make gen"), Fail(run, CASE_FAILS), Edit, Ok("make gen"), Ok(run)]);
    learn("logkit, edit then stale build then regenerate", &[Edit, Fail(run, CASE_FAILS), Ok("make gen"), Ok(run)]);

    let flag = "node --experimental-test-module-mocks --test";
    let book = learn("shopcart, node --test first", &[Fail("npm test", NO_SCRIPT), Fail("node --test", NO_MOCK), Ok(flag)]);
    hint(&book, "node --test", NO_MOCK, "same friction");
    hint(&book, "npm test", NO_SCRIPT, "same friction");
    hint(&book, "node --test test/tax.test.mjs", NO_MOCK, "same friction, one file");
    learn("shopcart, npm test then the flag", &[Fail("npm test", NO_SCRIPT), Ok(flag)]);
    learn("shopcart, a test fails for a real bug until the code is edited", &[Fail("node --test", NO_MOCK), Fail(flag, "ℹ fail 1"), Edit, Ok(flag)]);
}
