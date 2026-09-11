//! 闭集字符串枚举单一来源宏（ADR-0108；模式先例：`signals.rs` 的
//! `write_op_set!`，ADR-0102）。
//!
//! 清单即 enum 本体：同一 `(变体 => 字面量)` token 流同体展开五产物——
//! enum 本体、`ALL`（定长数组）、`as_str`（DB 存储/wire 序列化形状）、
//! `parse`（反序列化与 DB 读边界的严格映射）、`Display`（骑行 `as_str`）。
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
/// }
/// ```
///
/// `err_label` 用于 `parse` 未知值报错文案：`未知{label}: {s}（合法值: …）`，
/// 合法值清单由同一批字面量拼接生成（与 enum 顺序一致）。
macro_rules! closed_set {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $str:literal
            ),* $(,)?
        }
        err_label = $err_label:literal $(,)?
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

            /// 从闭集字符串严格解析；未知值报参数错误，文案附同源生成的
            /// 合法值清单。serde 反序列化与 DB 读边界（`FromSql`）复用本
            /// 函数（wire/DB 未知值即错，不静默映射）。
            pub fn parse(s: &str) -> $crate::error::Result<$name> {
                let value = match s {
                    $($str => $name::$variant,)*
                    other => {
                        return Err($crate::error::AppError::Invalid(format!(
                            concat!("未知", $err_label, ": {}（合法值: {}）"),
                            other,
                            [$($str),*].join("/")
                        )));
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

pub(crate) use closed_set;
