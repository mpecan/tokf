//! Proc-macro crate providing `#[crdb_test]`, a `CockroachDB`-compatible
//! replacement for `#[sqlx::test]`.
//!
//! `#[sqlx::test]` uses `pg_advisory_xact_lock()` internally for test database
//! coordination, which `CockroachDB` does not support. This macro performs the
//! same setup (create per-test DB, run migrations, inject `PgPool`, teardown)
//! without advisory locks.

use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, Ident, ItemFn, LitStr, Token, parse::Parse, parse::ParseStream};

struct Args {
    migrations: Option<String>,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut migrations = None;
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            if key == "migrations" {
                let _eq: Token![=] = input.parse()?;
                let path: LitStr = input.parse()?;
                migrations = Some(path.value());
            } else {
                return Err(syn::Error::new_spanned(key, "unknown argument"));
            }
            if !input.is_empty() {
                let _comma: Token![,] = input.parse()?;
            }
        }
        Ok(Self { migrations })
    }
}

/// Drop-in replacement for `#[sqlx::test]` that works with `CockroachDB`.
///
/// # Usage
///
/// ```ignore
/// #[crdb_test(migrations = "./migrations")]
/// async fn my_test(pool: PgPool) {
///     // test body — pool points at a fresh, migrated database
/// }
/// ```
///
/// The generated test:
/// 1. Reads `DATABASE_URL` from the environment (skips if unset)
/// 2. Creates a uniquely-named database (no advisory locks)
/// 3. Runs migrations with locking disabled
/// 4. Passes the pool to the test body
/// 5. Drops the database on completion
#[proc_macro_attribute]
pub fn crdb_test(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(attr as Args);
    let input = syn::parse_macro_input!(item as ItemFn);

    let fn_name = &input.sig.ident;
    let fn_vis = &input.vis;

    let pool_pat = match input.sig.inputs.first() {
        Some(FnArg::Typed(pat_type)) => &pat_type.pat,
        _ => {
            return syn::Error::new_spanned(
                &input.sig,
                "crdb_test function must take a PgPool argument",
            )
            .to_compile_error()
            .into();
        }
    };

    let migrations_path = args
        .migrations
        .unwrap_or_else(|| "./migrations".to_string());

    let fn_name_str = fn_name.to_string();
    let fn_body = &input.block;
    let setup = gen_setup(&fn_name_str, pool_pat, &migrations_path);

    let output = quote! {
        #[::tokio::test]
        #[ignore = "requires DATABASE_URL to be set"]
        #fn_vis async fn #fn_name() {
            #setup

            // ── Test body ──────────────────────────────────────────
            { #fn_body }

            // ── Teardown ───────────────────────────────────────────
            let _ = ::sqlx::Executor::execute(
                &mgmt_pool,
                ::sqlx::query(&format!(
                    "DROP DATABASE IF EXISTS \"{}\" CASCADE", db_name
                )),
            )
            .await;
        }
    };

    output.into()
}

fn gen_setup(
    fn_name_str: &str,
    pool_pat: &syn::Pat,
    migrations_path: &str,
) -> proc_macro2::TokenStream {
    quote! {
        let base_url = match ::std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => return,
        };

        let db_name = format!("_crdb_test_{}", #fn_name_str);

        let mgmt_pool = ::sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&base_url)
            .await
            .expect("crdb_test: failed to connect to management database");

        // Drop leftover test DB (from a previous crashed run), then create fresh
        let _ = ::sqlx::Executor::execute(
            &mgmt_pool,
            ::sqlx::query(&format!(
                "DROP DATABASE IF EXISTS \"{}\" CASCADE", db_name
            )),
        )
        .await;
        ::sqlx::Executor::execute(
            &mgmt_pool,
            ::sqlx::query(&format!("CREATE DATABASE \"{}\"", db_name)),
        )
        .await
        .expect("crdb_test: failed to create test database");

        // Build connection URL pointing at the test database
        let test_url = {
            let q = base_url.find('?').unwrap_or(base_url.len());
            let slash = base_url[..q]
                .rfind('/')
                .expect("DATABASE_URL must contain '/'");
            format!("{}/{}{}", &base_url[..slash], db_name, &base_url[q..])
        };

        let #pool_pat: ::sqlx::PgPool = ::sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&test_url)
            .await
            .expect("crdb_test: failed to connect to test database");

        // Run migrations with locking disabled (CockroachDB does not
        // support pg_advisory_lock / pg_advisory_xact_lock)
        {
            let mut migrator = ::sqlx::migrate!(#migrations_path);
            migrator.set_locking(false);
            migrator
                .run(&#pool_pat)
                .await
                .expect("crdb_test: migrations failed");
        }
    }
}
