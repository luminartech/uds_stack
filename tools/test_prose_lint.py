import os, sys, tempfile, unittest
sys.path.insert(0, os.path.dirname(__file__))
import prose_lint as pl

def write(dirpath, name, text):
    p = os.path.join(dirpath, name)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w") as f:
        f.write(text)
    return p

class HardPhrases(unittest.TestCase):
    def test_pull_request_reference_counts(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "a.rs", "/// Reclassified in PR #682.\nfn f() {}\n")
            files, hard, soft, hits, gated = pl.scan([d], [])
            self.assertEqual(hard["review-tool or PR/ticket reference"], 1)

    def test_repository_qualified_issue_reference_counts(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "a.rs", "// Capped since uds_stack#74.\n")
            files, hard, soft, hits, gated = pl.scan([d], [])
            self.assertEqual(hard["review-tool or PR/ticket reference"], 1)

    def test_need_id_is_not_a_ticket(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "a.rs", "/// Verifies UDSSVC_LLR_0042.\nfn f() {}\n")
            files, hard, soft, hits, gated = pl.scan([d], [])
            self.assertEqual(hard["review-tool or PR/ticket reference"], 0)

    def test_code_lines_are_not_scanned(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "a.rs", "let we = 1; // bound\n")
            files, hard, soft, hits, gated = pl.scan([d], [])
            self.assertEqual(hard["first person (we/our/us)"], 0)

class ScopeHeadings(unittest.TestCase):
    KEY = "'What this does not do' heading"

    def test_doc_comment_heading_counts(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "lib.rs", "//! ## What this crate does not do\n")
            files, hard, soft, hits, gated = pl.scan([d], [])
            self.assertEqual(hard[self.KEY], 1)

    def test_markdown_heading_counts(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "README.md", "# x\n\n## What this is not\n\nText.\n")
            files, hard, soft, hits, gated = pl.scan([d], [])
            self.assertEqual(hard[self.KEY], 1)

    def test_positive_heading_does_not_count(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "README.md", "## What this crate is\n")
            files, hard, soft, hits, gated = pl.scan([d], [])
            self.assertEqual(hard[self.KEY], 0)

    def test_plain_comment_starting_with_what_does_not_count(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "a.toml", "# what a reader sees. The tool does not touch it.\n")
            files, hard, soft, hits, gated = pl.scan([d], [])
            self.assertEqual(hard[self.KEY], 0)

    def test_markdown_body_and_fences_are_not_scanned(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "README.md", "We do this.\n\n```sh\n# What this does not do\n```\n")
            files, hard, soft, hits, gated = pl.scan([d], [])
            self.assertEqual(hard[self.KEY], 0)
            self.assertEqual(hard["first person (we/our/us)"], 0)

class Spelling(unittest.TestCase):
    KEY = pl.SPELLING

    def count(self, name, text):
        with tempfile.TemporaryDirectory() as d:
            write(d, name, text)
            files, hard, soft, hits, gated = pl.scan([d], [])
            return hard[self.KEY]

    def test_british_spelling_in_a_comment_counts(self):
        self.assertEqual(self.count("a.rs", "/// The behaviour once cancelled.\n"), 1)

    def test_british_spelling_in_markdown_body_counts(self):
        self.assertEqual(self.count("README.md", "# x\n\nIt recognises an acknowledgement.\n"), 1)

    def test_american_spelling_does_not_count(self):
        text = "/// The behavior, a cancellation, the programmed analyses, a neighbor.\n"
        self.assertEqual(self.count("a.rs", text), 0)

    def test_code_spans_and_identifiers_do_not_count(self):
        text = "/// Yields `ConnectionEvent::Unmodelled`, as `an_unmodelled_case` shows.\n"
        self.assertEqual(self.count("a.rs", text), 0)

    def test_fenced_markdown_and_the_changelog_do_not_count(self):
        self.assertEqual(self.count("README.md", "```text\nbehaviour\n```\n"), 0)
        self.assertEqual(self.count("CHANGELOG.md", "- The behaviour changed.\n"), 0)

class Gating(unittest.TestCase):
    def test_strict_prefix_gates_only_its_files(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "gated/a.rs", "// we do this\n")
            write(d, "free/b.rs", "// we do that\n")
            files, hard, soft, hits, gated = pl.scan([d], [os.path.join(d, "gated")])
            self.assertEqual(hard["first person (we/our/us)"], 2)
            self.assertEqual(gated["first person (we/our/us)"], 1)

class SoftSignals(unittest.TestCase):
    def test_twelve_consecutive_comment_lines_are_one_block(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "a.rs", "".join("// line %d\n" % i for i in range(12)) + "fn f() {}\n")
            files, hard, soft, hits, gated = pl.scan([d], [])
            self.assertEqual(soft["comment block of 12+ lines"], 1)

    def test_eleven_consecutive_comment_lines_are_not(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "a.rs", "".join("// line %d\n" % i for i in range(11)) + "fn f() {}\n")
            files, hard, soft, hits, gated = pl.scan([d], [])
            self.assertEqual(soft["comment block of 12+ lines"], 0)

class SkipDirs(unittest.TestCase):
    def test_fuzz_corpus_and_sphinx_output_are_skipped(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, "fuzz/corpus/x.rs", "// we copied this\n")
            write(d, "docs/_build/y.py", "# we generated this\n")
            files, hard, soft, hits, gated = pl.scan([d], [])
            self.assertEqual(files, 0)

if __name__ == "__main__":
    unittest.main()
