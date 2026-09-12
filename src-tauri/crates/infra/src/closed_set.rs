//! 闭集字符串枚举单一来源宏（ADR-0108；模式先例：`signals/write_op.rs` 的
//! `write_op_set!`，ADR-0102）。
//!
//! 清单即 enum 本体：同一 `(变体 => 字面量)` token 流同体展开五产物——
//! enum 本体、`ALL`（定长数组）、`as_str`（DB 存储/wire 序列化形状）、
//! `parse`（反序列化与 DB 读边界的严格映射，未知值报 ADR-0050 码化错误）、
//! `Display`（骑行 `as_str`）。
//! 「enum 新增变体漏登 `ALL` / 漏写 `as_str` / `parse` 臂」的漂移**不可表达**
//! （构造性保证，非「可检测」）；字符串字面量每变体只出现一次。
//!
//! 域自有接缝（serde 手写 impl、`FromSql`/`ToSql`、utoipa `PartialSchema`）
//! 不由宏生成：它们骑行宏产物（`as_str`/`parse`/`ALL`），本身不含第二份
//! 字面量事实。宏定义与调用分属两域（transaction / investment），故住址
//! 为本基础设施模块（ADR-0056：无域语义、被所有层消费），不同于
//! `write_op_set!` 的单域就地形态（其「局部性即卖点」前提是单一使用点）。
//!
//! 机制术语以 ADR-0108 为唯一解释处（ADR-0102 决策 6 先例），不进
//! `docs/contexts/`。

/// 闭集字符串枚举宏（ADR-0108）。调用形状：
///
/// ```ignore
/// closed_set! {
///     /// enum 级文档（原位透传）
///     #[derive(...)]
///     pub enum Kind {
///         /// 变体级文档（可选，原位透传）
///         Income => "income",
///     }
///     err_label = "交易类型",
///     err_code = "transaction.kind-unknown",
/// }
/// ```
///
/// `parse` 未知值报 ADR-0050 码化错误：文案 `未知{err_label}: {s}（合法值: …）`，
/// 合法值清单由同一批字面量拼接生成（与 enum 顺序一致）；`err_code` 是稳定错误码
/// （`<域>.<条件>`），`params` 为 `[未知值, 合法值清单]`——清单经 `{1}` 插值，不在
/// 前端码表里另抄一份（ADR-0108「消灭手抄清单」跨本地化边界保持）。`err_code` 为
/// 必填参数：第三枚举接入时漏码化不可表达。
// 宏体经 `$crate::error` 取基础设施错误类型；`#[macro_export]` 使宏项可经
// 本模块（`pub use closed_set;`）与根包（`pub use ledger_infra::closed_set;`）
// 分层再导出，域模块的 `crate::closed_set::closed_set` 引用路径零改动。
#[macro_export]
macro_rules! closed_set {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $str:literal
            ),* $(,)?
        }
        err_label = $err_label:literal,
        err_code = $err_code:literal $(,)?
    ) => {
        $(#[$enum_meta])*
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )*
        }

        impl $name {
            /// 全部变体（闭集清单，定长数组）：矩阵断言、SQL 片段生成与
            /// OpenAPI `enum_values` 按此遍历。由 `closed_set!` 宏从调用
            /// 清单同体展开（ADR-0108），与 enum 本体共享同一 token 流，
            /// 不存在第二份事实，漏登失败类不可表达。
            pub const ALL: [$name; [$( $name::$variant ),*].len()] = [
                $($name::$variant,)*
            ];

            /// 闭集字符串（DB 存储形状 + serde 序列化 + OpenAPI 枚举值，
            /// 三者同形同源）。
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $str,)*
                }
            }

            /// 从闭集字符串严格解析；未知值报 ADR-0050 码化参数错误（稳定
            /// `code`，`params` = `[未知值, 合法值清单]`），文案附同源生成的
            /// 合法值清单。serde 反序列化与 DB 读边界（`FromSql`）复用本
            /// 函数（wire/DB 未知值即错，不静默映射）。
            pub fn parse(s: &str) -> $crate::error::Result<$name> {
                let value = match s {
                    $($str => $name::$variant,)*
                    other => {
                        let legal = [$($str),*].join("/");
                        return Err($crate::error::AppError::codedp(
                            $err_code,
                            format!(
                                concat!("未知", $err_label, ": {}（合法值: {}）"),
                                other,
                                legal
                            ),
                            &[other, legal.as_str()],
                        ));
                    }
                };
                Ok(value)
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

// 宏经根包再导出（`pub use ledger_infra::closed_set;`）供域模块以
// `crate::closed_set::closed_set` 路径消费（issue #1088 基础设施 crate 归位）：
// 跨 crate 再导出要求宏项本身公开。
pub use closed_set;
