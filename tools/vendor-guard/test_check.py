"""Unit tests for the vendor guard. Forbidden tokens are assembled at run time so this file never contains one."""

import hashlib
import random
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check  # noqa: E402

RULES = check.load_rules(Path(__file__).resolve().parent / "denylist.toml")


def rules_with(sha: str) -> check.Rules:
    return check.Rules({**RULES.sha256, sha: "synthetic vendor file"}, RULES.paths, RULES.tokens,
                       RULES.image_allow, RULES.allow)


def cortex_image(sp: int = 0x2000_1EE8, reset: int = 0x0800_322D, size: int = 256) -> bytes:
    return sp.to_bytes(4, "little") + reset.to_bytes(4, "little") + bytes(size - 8)


class Tokens(unittest.TestCase):
    def rules_hit(self, text: str, path: str = "docs/notes.md") -> list[str]:
        return [f.rule for f in check.check_file(RULES, path, text.encode(), None)]

    def test_clean_text_passes(self) -> None:
        self.assertEqual(self.rules_hit("The balance loop runs at 496 Hz in the RTC interrupt.\n"), [])

    def test_decompiler_symbol(self) -> None:
        sym = "FUN" + "_" + "0800" + "3138"
        self.assertIn("token:decompiler-symbol", self.rules_hit(f"void {sym}(void);\n"))

    def test_decompiler_type(self) -> None:
        self.assertIn("token:decompiler-type", self.rules_hit("  " + "undefined" + "4 local_8;\n"))

    def test_disassembly_line(self) -> None:
        line = "0800" + "9b9c: fff77abf   b.w   #0x8009a94\n"
        self.assertIn("token:disassembly-listing", self.rules_hit(line))

    def test_firmware_address_in_prose_is_fine(self) -> None:
        self.assertEqual(self.rules_hit("The app links at 0x08003000 and the block sits at 0x08003140.\n"), [])


class PathsAndHashes(unittest.TestCase):
    def test_research_path(self) -> None:
        rules = [f.rule for f in check.check_file(RULES, "research/notes.md", b"x", None)]
        self.assertIn("path:research/**", rules)

    def test_ghidra_database(self) -> None:
        rules = [f.rule for f in check.check_file(RULES, "a/b/proj.rep/x/db.1.gbf", b"x", None)]
        self.assertTrue(any(r.startswith("path:") for r in rules))

    def test_exact_copy_by_sha256(self) -> None:
        blob = b"synthetic vendor blob"
        rules = rules_with(hashlib.sha256(blob).hexdigest())
        self.assertIn("sha256", [f.rule for f in check.check_file(rules, "x.bin", blob, None)])


class Images(unittest.TestCase):
    def test_cortex_m_image_flagged(self) -> None:
        self.assertIn("image", [f.rule for f in check.check_file(RULES, "assets/fw.bin", cortex_image(), None)])

    def test_cortex_m_image_allowed_in_build_output(self) -> None:
        self.assertEqual(check.check_file(RULES, "firmware/build/target-clean/fw.bin", cortex_image(), None), [])

    def test_even_reset_vector_is_not_an_image(self) -> None:
        self.assertFalse(check.looks_like_cortex_m_image(cortex_image(reset=0x0800_322C)))


class DeepCheck(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        rnd = random.Random(1234)
        cls.stock_blob = bytes(rnd.randrange(256) for _ in range(4096))
        cls.tmp = tempfile.TemporaryDirectory()
        stock_path = Path(cls.tmp.name) / "stock.bin"
        stock_path.write_bytes(cls.stock_blob)
        cls.index = check.StockIndex([stock_path])

    @classmethod
    def tearDownClass(cls) -> None:
        cls.tmp.cleanup()

    def hits(self, blob: bytes) -> list[str]:
        return [f.rule for f in check.check_file(RULES, "data/x.dat", blob, self.index)]

    def test_raw_copy_at_odd_offset(self) -> None:
        self.assertIn("stock", self.hits(b"\x01\x02\x03" + self.stock_blob[1001:1001 + 20]))

    def test_hex_text_copy(self) -> None:
        text = " ".join(f"{b:02x}" for b in self.stock_blob[2000:2024]).encode()
        self.assertIn("stock", self.hits(b"dump: " + text + b"\n"))

    def test_c_array_copy(self) -> None:
        text = ", ".join(f"0x{b:02X}" for b in self.stock_blob[300:330]).encode()
        self.assertIn("stock", self.hits(b"static const uint8_t t[] = {" + text + b"};\n"))

    def test_padding_does_not_match(self) -> None:
        self.assertEqual(check.StockIndex.first_hit(self.index, bytes(64)), None)

    def test_unrelated_bytes_pass(self) -> None:
        other = bytes(random.Random(99).randrange(256) for _ in range(4096))
        self.assertNotIn("stock", self.hits(other))


class Allowlist(unittest.TestCase):
    def test_allow_waives_one_rule(self) -> None:
        rules = check.Rules(RULES.sha256, RULES.paths, RULES.tokens, RULES.image_allow,
                            [("docs/spec/listing.md", {"token:disassembly-listing"}, "format example")])
        line = ("0800" + "9b9c: fff77abf   b.w   #0x8009a94\n").encode()
        self.assertEqual(check.check_file(rules, "docs/spec/listing.md", line, None), [])
        self.assertNotEqual(check.check_file(rules, "docs/other.md", line, None), [])


if __name__ == "__main__":
    unittest.main()
