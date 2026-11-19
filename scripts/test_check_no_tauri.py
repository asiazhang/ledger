import os
from pathlib import Path

import pytest

from check_no_tauri import scan_cargo_manifests

# 全仓无 tauri 守门的包装测试（删除即变红，ADR-0087 断言强度）：夹具仓库根
# 按扫描根同构摆放，断言「Cargo 清单 tauri 声明」命中即红、清除后零命中；
# 注释提及不误报（掩码语义）；空集拒绝假绿。前端死路径面（原面②）已随
# Web 参照前端退役删除（#2171/#2172），本测试只辖 Cargo 清单面。


@pytest.fixture()
def fixture_root(tmp_path: Path) -> str:
    root = str(tmp_path / "check-no-tauri-fixture")
    os.makedirs(os.path.join(root, "src-app/crates/infra"))
    Path(root, "src-app/Cargo.toml").write_text(
        '[package]\nname = "nsh-app"\n# 注释提及 tauri 不算（掩码语义）\n', encoding="utf-8"
    )
    Path(root, "src-app/crates/infra/Cargo.toml").write_text(
        '[package]\nname = "ledger-infra"\n', encoding="utf-8"
    )
    return root


def write(rel: str, content: str, root: str) -> None:
    abs_path = os.path.join(root, rel)
    os.makedirs(os.path.dirname(abs_path), exist_ok=True)
    Path(abs_path).write_text(content, encoding="utf-8")


class Test全仓无Tauri守门:
    def test_manifest_declaration_hits_and_comment_masked(self, fixture_root: str) -> None:
        assert scan_cargo_manifests(fixture_root) == []
        write(
            "src-app/crates/infra/Cargo.toml",
            '[package]\nname = "ledger-infra"\ntauri = { version = "2", optional = true }\n'
            'tauri-emit = ["dep:tauri"]\n',
            fixture_root,
        )
        hits = scan_cargo_manifests(fixture_root)
        assert len(hits) == 2
        assert hits[0].file == "src-app/crates/infra/Cargo.toml"
        assert hits[0].line == 3
        # 注释行不算（TOML 注释掩码）
        write(
            "src-app/crates/infra/Cargo.toml",
            '[package]\nname = "ledger-infra"\n# tauri-emit 门已退役\n',
            fixture_root,
        )
        assert scan_cargo_manifests(fixture_root) == []

    def test_real_repo_zero_hits(self) -> None:
        # 直接对真实仓库跑扫描（pytest cwd = 仓库根）：任何残留即本测试红
        # （与 CI 门同判，删除即变红的负例已在上方用例覆盖）。
        assert scan_cargo_manifests(".") == [], "真实仓库存在 tauri 残留"
