import re

import pytest

from gate_primitives import (
    DirEntry,
    RUST_EXTENSIONS,
    WalkTextOptions,
    has_extension,
    line_at,
    mask_non_code,
    read_dir_entries,
    walk_text_files,
)

# 被测对象是守门家族共享原语库 scripts/gate_primitives.py（issue #1680 库归库、
# 门归门）：原语级测试面归库自身，掩码与遍历的正确性不再借结构守门的通过来间接
# 证明。
# - Rust 词法掩码双源防漂移语料与 line_at / walk_text_files 用例自
#   check_structure 的测试迁入（#1433 / #1625「直测导出共享面」先例随库迁移，
#   删除即变红判据不变：从库删掉一个导出 → 消费它的门 uv run 启动即崩 → 包装
#   测试红 + 类型检查红）；
# - TS/Vue 注释掩码用例自 ts-comment-mask 测试迁入（#1481，模块已并入本库）。
# （仓库根 = pytest 进程 cwd——守门测试自仓库根发起；原 repo_root 助手
#  has-command-line 测试助手已随守门挂载登记收编、消费清零而退役，issue #1682）

from pathlib import Path

REPO_ROOT = Path.cwd()


def read(rel: str) -> str:
    return (REPO_ROOT / rel).read_text(encoding="utf-8")


class TestMaskNonCodeCorpus:
    """mask_non_code 双源防漂移语料（issue #1433）。"""

    CORPUS = REPO_ROOT / "scripts" / "fixtures" / "rust-mask-corpus.rs"
    EXPECTED = REPO_ROOT / "scripts" / "fixtures" / "rust-mask-corpus.expected.txt"

    def test_mask_output_matches_shared_corpus_expectation(self) -> None:
        # 与 Rust 侧唯一实现（src-app/src/test_support/scan.rs 的 mask_non_code）
        # 消费同一夹具：语料输入 + 期望输出双文件共享，任一侧单独改词法规则即
        # 本测或 Rust 语料测试红。（pytest 下取进程 cwd = 仓库根定位夹具，同上款先例）
        corpus = self.CORPUS.read_text(encoding="utf-8")
        expected = self.EXPECTED.read_text(encoding="utf-8")
        assert mask_non_code(corpus) == expected

    def test_keep_literals_matches_shared_corpus_expectation(self) -> None:
        # keepLiterals=True 与共享语料期望全等（脚本侧单侧锁定：Rust 侧无
        # keepLiterals 是 scan.rs 头注登记在案的不对称）
        keep_expected = (
            REPO_ROOT / "scripts" / "fixtures" / "rust-mask-corpus.keepLiterals.expected.txt"
        ).read_text(encoding="utf-8")
        assert mask_non_code(self.CORPUS.read_text(encoding="utf-8"), True) == keep_expected

    def test_keep_literals_masks_only_comments(self) -> None:
        # keepLiterals=True 只掩注释、保留字符串字面量（脚本侧扩展形态，语料外的直接断言）
        src = '// comment\nlet s = "keep";\n'
        assert mask_non_code(src, True) == '          \nlet s = "keep";\n'


class TestLineAtAndWalkTextFiles:
    """守门家族共享扫描单点：命中行定位 line_at + 文本面遍历 walk_text_files
    （issue #1625）：守门家族共享面（供各守门直接消费）：行号定位与目录遍历收口
    单点，扩展名闭集与豁免面属各守门政策经参数注入。
    """

    def test_line_at_counts_newlines_before_match(self) -> None:
        # line_at：按命中点之前的换行数计行（1 起算），index None 按文首计
        source = "\n".join(["const a = 1;", "", "const url = 'https://x.example';"])
        m = re.search(r"x\.example", source)
        assert m is not None
        assert line_at(source, m.start()) == 3
        assert line_at("abc def", None) == 1
        assert line_at("abc def", 0) == 1

    def test_walk_text_files_filters_and_prunes_with_deterministic_order(
        self, tmp_path: Path
    ) -> None:
        # walk_text_files：扩展名闭集过滤 + 目录名剪枝 + 相对路径剪枝，排序确定
        dir_path = tmp_path
        (dir_path / "src" / "nested").mkdir(parents=True)
        (dir_path / "node_modules").mkdir()
        (dir_path / "CHANGELOG.md").write_text("# history\n", encoding="utf-8")
        (dir_path / "README").write_text("无扩展名文件不参与文本面", encoding="utf-8")
        (dir_path / "src" / "b.ts").write_text("export const b = 1;\n", encoding="utf-8")
        (dir_path / "src" / "skip-me.ts").write_text("export const s = 1;\n", encoding="utf-8")
        (dir_path / "src" / "nested" / "a.rs").write_text("pub fn a() {}\n", encoding="utf-8")
        (dir_path / "src" / "nested" / "a.rs.bak").write_text("junk", encoding="utf-8")
        (dir_path / "node_modules" / "dep.ts").write_text(
            "export const dep = 1;\n", encoding="utf-8"
        )

        options = WalkTextOptions(
            extensions=frozenset({".ts", ".rs", ".md"}),
            skip_dirs=frozenset({"node_modules"}),
            skip_files=frozenset({"src/skip-me.ts"}),
        )
        files = walk_text_files(str(dir_path), "", options)
        assert [f.rel for f in files] == ["CHANGELOG.md", "src/b.ts", "src/nested/a.rs"]
        # abs 与 rel 指向同一文件（读文件用 abs、报文定位用 rel）
        assert files[2].abs == str(dir_path / "src" / "nested" / "a.rs")
        # 排序确定性：同一目录重复调用输出全等
        assert [f.rel for f in walk_text_files(str(dir_path), "", options)] == [
            f.rel for f in files
        ]

    def test_walk_text_files_relbase_prefix_normalization(self, tmp_path: Path) -> None:
        # walk_text_files：relBase 前缀归一 rel 路径（扫描根 ≠ 仓库根的守门形态）
        # check-infra-dml 形态：扫描根 = join(srcTauri, INFRA_SRC_REL)，relBase 同值——
        # rel 不随夹具所在绝对路径漂移，始终以声明前缀归一。
        scan_root = tmp_path / "crates" / "infra" / "src"
        (scan_root / "db").mkdir(parents=True)
        (scan_root / "db" / "migrate.rs").write_text("pub fn migrate() {}\n", encoding="utf-8")

        files = walk_text_files(
            str(scan_root), "crates/infra/src", WalkTextOptions(extensions=frozenset({".rs"}))
        )
        assert [f.rel for f in files] == ["crates/infra/src/db/migrate.rs"]


# 一层列举与扩展名匹配的原语（#1742 收口）：平铺 readdir 与递归遍历形状不同构，
# 收口为家族统一的单层语义——localeCompare 排序 + dirent 类型投影；has_extension
# 与 walk_text_files 内部匹配同款，内联 endsWith(".rs") 残余（test-exec ×3、
# check-structure ×1）换库扩展名闭集消费。
class TestReadDirEntriesAndHasExtension:
    def test_read_dir_entries_single_level_with_dirent_projection(self, tmp_path: Path) -> None:
        # readDirEntries：一层列举不递归，dirent 类型投影（文件/目录）
        dir_path = tmp_path
        (dir_path / "subdir" / "nested").mkdir(parents=True)
        (dir_path / "a.rs").write_text("pub fn a() {}\n", encoding="utf-8")
        (dir_path / "subdir" / "inner.rs").write_text("pub fn inner() {}\n", encoding="utf-8")
        (dir_path / "subdir" / "nested" / "deep.rs").write_text(
            "pub fn deep() {}\n", encoding="utf-8"
        )

        entries = read_dir_entries(str(dir_path))
        # 一层：子目录只出现自身条目，不下降（嵌套内容不出现）；排序见下款
        assert [e.name for e in entries] == ["a.rs", "subdir"]
        assert entries[0] == DirEntry(name="a.rs", is_directory=False, is_file=True)
        assert entries[1] == DirEntry(name="subdir", is_directory=True, is_file=False)

    def test_read_dir_entries_locale_compare_order(self, tmp_path: Path) -> None:
        # readDirEntries：localeCompare 排序（家族统一行为，与 walk_text_files 同款）
        dir_path = tmp_path
        for name in ["B", "a", "c"]:
            (dir_path / name).write_text("x", encoding="utf-8")
        # localeCompare 大小写不敏感主级比较：a < B；plain sort 按码点会得 ["B", "a", "c"]
        assert [e.name for e in read_dir_entries(str(dir_path))] == ["a", "B", "c"]

    def test_read_dir_entries_does_not_follow_dir_symlink(self, tmp_path: Path) -> None:
        # readDirEntries：目录符号链接不跟随（isFile/isDirectory 均 false，与
        # walk_text_files 同语义）
        dir_path = tmp_path
        (dir_path / "real").mkdir()
        (dir_path / "link").symlink_to(dir_path / "real")

        entries = read_dir_entries(str(dir_path))
        link = [e for e in entries if e.name == "link"]
        assert link == [DirEntry(name="link", is_directory=False, is_file=False)]

    def test_read_dir_entries_raises_on_missing_dir(self, tmp_path: Path) -> None:
        # readDirEntries：目录不存在按 scandir 原样抛错（不静默空集，调用侧 exists
        # 守卫保留）
        with pytest.raises(OSError):
            read_dir_entries(str(tmp_path / "read-dir-entries-missing-nope"))

    def test_has_extension_equivalent_to_ends_with(self) -> None:
        # hasExtension：点扩展名闭集匹配与 endsWith 全等（残余内联 endsWith 收口的
        # 等价性判据）
        names = [".rs", "a.rs", "lib.rs", "rs", "a.b.rs", "a.RS", "a.rs.txt", "", ".r", "a."]
        for name in names:
            assert has_extension(name, RUST_EXTENSIONS) == name.endswith(".rs")
        assert has_extension("lib.rs", RUST_EXTENSIONS) is True
        assert has_extension("main", RUST_EXTENSIONS) is False

    def test_has_extension_multi_extension_set(self) -> None:
        # hasExtension：多扩展名集合成员判定
        exts = frozenset({".json", ".ts"})
        assert has_extension("errors.json", exts) is True
        assert has_extension("app.ts", exts) is True
        assert has_extension("a.json5", exts) is False
        assert has_extension("json", exts) is False


# Rust 词法掩码单源收敛（#1680）：mask_non_code 的 Python 消费名单锁定——任一
# 消费方摘掉库 import、回退本地副本（或新脚本绕开库自建第四份实现），下面的断言
# 即红；语料期望只锁规则本身，名单把「谁在消费同一实现」也钉住，防分歧潜伏。
# #1741 起名单含弱形态注释处理器的收口方：check-i18n-keys（码化构造点提取腿，
# 原自持 stripRustComments）——摘掉 import 即红；弱形态回潮的守门在两门各自的
# 包装夹具（掩码权威形态旧红新绿），本名单只钉「谁在消费同一实现」。
# check-commands / check-background-services 消费方随 #2083 壳退役移出名单。
class TestSingleSourceMask:
    CONSUMERS = [
        "check_structure.py",
        "check_infra_dml.py",
        "test_exec.py",
        "check_test_support.py",
        "check_i18n_keys.py",
    ]

    @pytest.mark.parametrize("file", CONSUMERS)
    def test_consumer_imports_mask_from_shared_library(self, file: str) -> None:
        src = read(f"scripts/{file}")
        assert re.search(
            r"from\s+gate_primitives\s+import\s*\(?\s*[^)]*?\bmask_non_code\b", src
        ), f"{file} 未从共享库消费 mask_non_code"
        # 本地重定义（def 副本）同样即红
        assert not re.search(r"\bdef\s+mask_non_code\b", src), f"{file} 本地重定义 mask_non_code"
