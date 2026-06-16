//! Paporot 主入口 —— AI 生成软件的行为版本控制与审计系统

mod agent;
mod analysis;
mod cli;
mod commands;
mod config;
mod graph;
mod llm;
mod prompts;
mod storage;
mod trace;
mod trajectory;
mod types;

use clap::Parser;

use crate::trace::types::RedactConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();

    // 加载配置
    let config = config::Config::load_or_default(&cli.config);

    // 初始化 Agent
    let agent = agent::Agent::new(config.clone());

    // 初始化存储
    agent.storage.init()?;

    match cli.command {
        cli::Commands::Snapshot { action } => match action {
            cli::SnapshotAction::Create {
                diff_range,
                message,
                prd,
                diff_file,
                output_dir,
            } => {
                commands::snapshot::run(
                    &agent,
                    &diff_range,
                    &message,
                    prd.as_deref(),
                    diff_file.as_deref(),
                    &output_dir,
                )
                .await?;
            }
        },

        cli::Commands::Diff { from, to, format } => {
            commands::diff::run(&agent, from.as_deref(), to.as_deref(), &format).await?;
        }

        cli::Commands::Coverage { prd, version } => {
            commands::coverage::run(&agent, prd.as_deref(), version.as_deref()).await?;
        }

        cli::Commands::Regression { from, to } => {
            commands::regression::run(&agent, from.as_deref(), to.as_deref()).await?;
        }

        cli::Commands::Risk { version } => {
            commands::risk::run(&agent, version.as_deref()).await?;
        }

        cli::Commands::Review { diff_source, prd } => {
            commands::review::run(&agent, diff_source.as_deref(), prd.as_deref()).await?;
        }

        cli::Commands::Version => {
            commands::version::run(&agent).await?;
        }

        cli::Commands::Status => {
            commands::version::status(&agent).await?;
        }

        cli::Commands::Graph { action } => match action {
            cli::GraphAction::Show { version, capability, depth } => {
                commands::graph::show(&agent, version.as_deref(), capability.as_deref(), depth)?;
            }
            cli::GraphAction::Impact { capability } => {
                commands::graph::impact(&agent, &capability)?;
            }
            cli::GraphAction::Evolution { capability } => {
                commands::graph::evolution(&agent, &capability)?;
            }
            cli::GraphAction::Cycles => {
                commands::graph::cycles(&agent)?;
            }
            cli::GraphAction::Module { name } => {
                commands::graph::module(&agent, &name)?;
            }
        },

        cli::Commands::Feedback { action } => {
            use crate::types::{FeedbackStore, FEEDBACK_DIR, REVIEWS_FILE};
            let reviews_path = std::path::Path::new(FEEDBACK_DIR).join(REVIEWS_FILE);
            let mut feedback = FeedbackStore::load_or_new(&reviews_path)
                .unwrap_or_else(|_| FeedbackStore {
                    reviews: vec![],
                    stats: crate::types::FeedbackStats::default(),
                });

            match action {
                cli::FeedbackAction::Approve { capability, version, reviewer, comment } => {
                    commands::feedback::approve(&mut feedback, &capability, &version, &reviewer, comment.as_deref())?;
                }
                cli::FeedbackAction::Reject { capability, version, reviewer, reason } => {
                    commands::feedback::reject(&mut feedback, &capability, &version, &reviewer, reason.as_deref())?;
                }
                cli::FeedbackAction::Correct { capability, version, reviewer, name, desc, comment } => {
                    commands::feedback::correct(&mut feedback, &capability, &version, &reviewer, &name, &desc, comment.as_deref())?;
                }
                cli::FeedbackAction::Flag { capability, version, reviewer, note } => {
                    commands::feedback::flag(&mut feedback, &capability, &version, &reviewer, note.as_deref())?;
                }
                cli::FeedbackAction::Show { capability } => {
                    commands::feedback::show(&feedback, capability.as_deref())?;
                    return Ok(());
                }
                cli::FeedbackAction::Stats => {
                    commands::feedback::stats(&feedback)?;
                    return Ok(());
                }
            }
            feedback.save(&reviews_path)?;
        }

        cli::Commands::Testmap { action } => {
            use crate::types::{FEEDBACK_DIR, TESTMAP_FILE};
            let testmap_path = std::path::Path::new(FEEDBACK_DIR).join(TESTMAP_FILE);
            let mut testmap = crate::types::TestMapStore::load_or_new(&testmap_path)
                .unwrap_or_else(|_| crate::types::TestMapStore {
                    mappings: vec![],
                    stats: crate::types::TestMapStats::default(),
                });

            match action {
                cli::TestmapAction::Scan { version, diff } => {
                    let diff_content = if let Some(d) = diff {
                        std::fs::read_to_string(&d).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    commands::testmap::scan(&mut testmap, &diff_content, &version)?;
                }
                cli::TestmapAction::Add { capability, test_file, test_name, status, framework, source } => {
                    commands::testmap::add(&mut testmap, &capability, &test_file, &test_name, &status, framework.as_deref(), &source)?;
                }
                cli::TestmapAction::Show { capability } => {
                    commands::testmap::show(&testmap, capability.as_deref())?;
                    return Ok(());
                }
                cli::TestmapAction::Stats => {
                    commands::testmap::stats(&testmap)?;
                    return Ok(());
                }
                cli::TestmapAction::Verify { capability } => {
                    commands::testmap::verify(&testmap, &capability)?;
                    return Ok(());
                }
            }
            testmap.save(&testmap_path)?;
        },

        cli::Commands::Trace { action } => {
            let storage = crate::trace::storage::TraceStorage::new(
                std::path::Path::new(".Paporot"),
            );
            storage.init()?;

            match action {
                cli::TraceAction::Import { file, adapter } => {
                    // 检查自动脱敏配置
                    let auto_redact = if config.trace.auto_redact {
                        Some(commands::trace::make_redact_config_from_trace_config(
                            &config.trace,
                        ))
                    } else {
                        None
                    };
                    let result = commands::trace::cmd_import(
                        &storage,
                        &file,
                        adapter.as_deref(),
                        auto_redact.as_ref(),
                    )?;
                    println!("Paporot Trace Import");
                    println!("  source       : {}", result.source_path);
                    println!(
                        "  adapter      : {}{}",
                        result.adapter,
                        if result.auto_detected {
                            " (auto-detected)"
                        } else {
                            ""
                        }
                    );
                    println!(
                        "  traces       : {} imported, {} skipped",
                        result.imported.len(),
                        result.skipped_count
                    );
                    for s in result.skip_reasons.iter().take(5) {
                        eprintln!("  [skip] {}", s);
                    }
                    println!("  ── Imported ──");
                    for t in &result.imported {
                        println!(
                            "  {}  prompt: \"{}\"  tools: {}  tokens: {}",
                            t.id, t.prompt_preview, t.tool_call_count, t.total_tokens
                        );
                    }
                }
                cli::TraceAction::List {
                    session,
                    tool,
                    tag,
                    capability,
                    from,
                    to,
                    limit,
                    offset,
                } => {
                    let filter = crate::trace::types::TraceFilter {
                        session_id: session,
                        tool_name: tool,
                        tag,
                        capability_id: capability,
                        from_date: from,
                        to_date: to,
                        limit,
                        offset,
                        ..Default::default()
                    };
                    let results = commands::trace::cmd_list(&storage, filter)?;
                    if results.is_empty() {
                        println!("No traces found.");
                    } else {
                        for s in &results {
                            println!(
                                "{}  session: {}  tools: {}  tokens: {}  {}",
                                s.id,
                                s.session_id,
                                s.tool_names.join(","),
                                s.total_tokens,
                                s.started_at
                            );
                        }
                    }
                }
                cli::TraceAction::Show { trace_id, format } => {
                    let fmt = match format.to_lowercase().as_str() {
                        "json" => commands::trace::ShowFormat::Json,
                        "summary" => commands::trace::ShowFormat::Summary,
                        _ => commands::trace::ShowFormat::Full,
                    };
                    commands::trace::cmd_show(&storage, &trace_id, fmt)?;
                }
                cli::TraceAction::Delete { trace_id } => {
                    commands::trace::cmd_delete(&storage, &trace_id)?;
                    println!("Trace {} deleted (soft delete)", trace_id);
                }
                cli::TraceAction::Link { trace_id, cap } => {
                    commands::trace::cmd_link(&storage, &trace_id, &cap)?;
                    println!("Linked trace {} -> capability {}", trace_id, cap);
                }
                cli::TraceAction::Unlink { trace_id, cap } => {
                    commands::trace::cmd_unlink(&storage, &trace_id, &cap)?;
                    println!("Unlinked trace {} -> capability {}", trace_id, cap);
                }
                cli::TraceAction::Redact { trace_id } => {
                    let config = RedactConfig::default();
                    commands::trace::cmd_redact(&storage, &trace_id, &config)?;
                    println!("Trace {} redacted", trace_id);
                }
                cli::TraceAction::Adapter {
                    action: cli::AdapterAction::List,
                } => {
                    let adapters = commands::trace::cmd_adapter_list()?;
                    for a in &adapters {
                        println!("  {:<16} v{:<8} {}", a.name, a.version, a.description);
                    }
                }
            }
        },
        cli::Commands::Trajectory { action } => {
            let base_dir = std::path::PathBuf::from(".Paporot");
            match action {
                cli::TrajectoryAction::Diff {
                    capability, trace_a, trace_b, format, output,
                } => {
                    let storage = crate::trace::storage::TraceStorage::new(&base_dir);
                    storage.init()
                        .map_err(|e| anyhow::anyhow!("Storage init failed: {:?}", e))?;
                    commands::trajectory::run_diff(
                        &storage, &base_dir,
                        capability, trace_a, trace_b, &format, output,
                    )?;
                }
                cli::TrajectoryAction::List => {
                    commands::trajectory::run_list(&base_dir)?;
                }
                cli::TrajectoryAction::Show { diff_id } => {
                    commands::trajectory::run_show(&base_dir, &diff_id)?;
                }
            }
        },
        cli::Commands::State { action } => {
            let base_dir = std::path::PathBuf::from(".Paporot");
            let storage = crate::trace::storage::TraceStorage::new(&base_dir);
            storage.init()
                .map_err(|e| anyhow::anyhow!("Storage init failed: {:?}", e))?;
            match action {
                cli::StateAction::Build { trace } => {
                    commands::state::run_build(&storage, &trace)
                        .map_err(|e| anyhow::anyhow!("State build: {}", e))?;
                }
                cli::StateAction::Show { trace_id, format } => {
                    commands::state::run_show(&storage, &trace_id, &format, &base_dir)
                        .map_err(|e| anyhow::anyhow!("State show: {}", e))?;
                }
                cli::StateAction::Diff { trace_a, trace_b, format, .. } => {
                    let (a, b) = if let (Some(ta), Some(tb)) = (trace_a, trace_b) {
                        (ta, tb)
                    } else {
                        anyhow::bail!("Both --trace-a and --trace-b must be provided");
                    };
                    commands::state::run_diff(&storage, &a, &b, &format)
                        .map_err(|e| anyhow::anyhow!("State diff: {}", e))?;
                }
                cli::StateAction::Eval { trace } => {
                    commands::state::run_eval(&storage, &trace)
                        .map_err(|e| anyhow::anyhow!("State eval: {}", e))?;
                }
            }
        },
        cli::Commands::TrajectoryVector { action } => {
            let base_dir = std::path::PathBuf::from(".Paporot");
            let storage = crate::trace::storage::TraceStorage::new(&base_dir);
            storage.init()
                .map_err(|e| anyhow::anyhow!("Storage init failed: {:?}", e))?;

            match action {
                cli::TrajectoryVectorAction::Build { trace, output } => {
                    commands::trajectory_vector::run_vector_build(&storage, &trace, output)
                        .map_err(|e| anyhow::anyhow!("Vector build: {}", e))?;
                }
                cli::TrajectoryVectorAction::Diff { v1, v2 } => {
                    commands::trajectory_vector::run_vector_diff(&v1, &v2)
                        .map_err(|e| anyhow::anyhow!("Vector diff: {}", e))?;
                }
                cli::TrajectoryVectorAction::Cluster { traces } => {
                    commands::trajectory_vector::run_cluster_analyze(&storage, &traces)
                        .map_err(|e| anyhow::anyhow!("Cluster: {}", e))?;
                }
                cli::TrajectoryVectorAction::Anomaly { traces } => {
                    commands::trajectory_vector::run_anomaly_detect(&storage, &traces)
                        .map_err(|e| anyhow::anyhow!("Anomaly: {}", e))?;
                }
            }
        },
        cli::Commands::Coupling { action } => {
            let base_dir = std::path::PathBuf::from(".Paporot");
            let storage = crate::trace::storage::TraceStorage::new(&base_dir);
            storage.init()
                .map_err(|e| anyhow::anyhow!("Storage init failed: {:?}", e))?;

            match action {
                cli::CouplingAction::Build { pairs, output } => {
                    let parsed: Vec<(String, String)> = parse_pairs(&pairs)?;
                    commands::coupling::run_coupling_build(&storage, &parsed, output)
                        .map_err(|e| anyhow::anyhow!("Coupling build: {}", e))?;
                }
                cli::CouplingAction::Analyze { cap, pairs } => {
                    let parsed: Vec<(String, String)> = parse_pairs(&pairs)?;
                    commands::coupling::run_coupling_analyze(&storage, &parsed, &cap)
                        .map_err(|e| anyhow::anyhow!("Coupling analyze: {}", e))?;
                }
                cli::CouplingAction::Export { pairs, format } => {
                    let parsed: Vec<(String, String)> = parse_pairs(&pairs)?;
                    commands::coupling::run_coupling_graph_export(&storage, &parsed, &format)
                        .map_err(|e| anyhow::anyhow!("Coupling export: {}", e))?;
                }
                cli::CouplingAction::Impact { cap, pairs } => {
                    let parsed: Vec<(String, String)> = parse_pairs(&pairs)?;
                    commands::coupling::run_coupling_analyze(&storage, &parsed, &cap)
                        .map_err(|e| anyhow::anyhow!("Coupling impact: {}", e))?;
                }
            }
        },
    }

    Ok(())
}

/// Parse "trace_id:cap_id" pairs from CLI args.
fn parse_pairs(pairs: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    let mut result = Vec::new();
    for p in pairs {
        let parts: Vec<&str> = p.splitn(2, ':').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid pair format '{}'. Expected trace_id:cap_id", p);
        }
        result.push((parts[0].to_string(), parts[1].to_string()));
    }
    Ok(result)
}
