mod benchmark;
mod broker;
mod codex;
mod github;
mod model;

use anyhow::{bail, Result};
use github::ReviewTarget;
use model::{Provider, ReviewRequest};
use std::env;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let provider = Provider::from_env()?;
    let api_key = provider.api_key()?;
    let model = provider.model()?;
    let (passes, threshold) = model::voting_config()?;

    if args.get(1).map(String::as_str) == Some("--benchmark") {
        let path = args
            .get(2)
            .ok_or_else(|| anyhow::anyhow!("usage: second-opinion --benchmark <suite.json>"))?;
        return benchmark::run(
            path,
            provider,
            api_key.as_deref(),
            &model,
            passes,
            threshold,
        );
    }
    if args.len() > 1 {
        bail!(
            "unknown argument '{}'; use --benchmark <suite.json>",
            args[1]
        );
    }

    let github_token = github::required_env("GITHUB_TOKEN")?;
    let repo = github::required_env("GITHUB_REPOSITORY")?;
    let target = github::detect_review_target()?;
    match &target {
        ReviewTarget::PullRequest { number } => {
            eprintln!(
                "Reviewing {repo}#{number} with {}/{model} ({passes} pass(es), threshold {threshold})",
                provider.name()
            );
        }
        ReviewTarget::Commit { sha, .. } => {
            eprintln!(
                "Reviewing {repo}@{sha} with {}/{model} ({passes} pass(es), threshold {threshold})",
                provider.name()
            );
        }
    }

    run_review(
        &github_token,
        &repo,
        &target,
        provider,
        api_key.as_deref(),
        &model,
        passes,
        threshold,
    )
}

fn run_review(
    github_token: &str,
    repo: &str,
    target: &ReviewTarget,
    provider: Provider,
    api_key: Option<&str>,
    model: &str,
    passes: usize,
    threshold: usize,
) -> Result<()> {
    if let ReviewTarget::Commit { sha, .. } = target {
        if github::commit_has_open_pull(github_token, repo, sha)? {
            eprintln!(
                "Commit {sha} is already the head of an open pull request; skipping push review."
            );
            return Ok(());
        }
    }
    if let ReviewTarget::PullRequest { number } = target {
        if !github::head_is_expected(github_token, repo, *number)? {
            eprintln!(
                "Pull request head changed before review input was loaded; skipping stale job."
            );
            return Ok(());
        }
        if github::review_run_already_posted(github_token, repo, *number)? {
            eprintln!("This pull request revision already has a completed second-opinion review.");
            return Ok(());
        }
    }

    let input = github::load_review_input(github_token, repo, target)?;
    if input.diff.trim().is_empty() {
        eprintln!("Empty diff, nothing to review.");
        return Ok(());
    }
    if let ReviewTarget::PullRequest { number } = target {
        if !github::head_is_expected(github_token, repo, *number)? {
            eprintln!(
                "Pull request head changed while review input was loaded; skipping stale job."
            );
            return Ok(());
        }
    }

    let request = ReviewRequest {
        repo,
        diff: &input.diff,
        context: &input.context,
        rules: &input.rules,
    };
    let mut review = model::run_consensus(provider, api_key, model, &request, passes, threshold)?;
    if let ReviewTarget::PullRequest { number } = target {
        if !github::head_is_expected(github_token, repo, *number)? {
            eprintln!("Pull request head changed during model execution; discarding stale review.");
            return Ok(());
        }
        let existing = match github::existing_fingerprints(github_token, repo, *number) {
            Ok(existing) => existing,
            Err(error) => {
                eprintln!("Could not load existing review fingerprints: {error:#}");
                Default::default()
            }
        };
        let before = review.findings.len();
        review
            .findings
            .retain(|finding| !existing.contains(&github::finding_fingerprint(finding)));
        let skipped = before - review.findings.len();
        if skipped > 0 {
            review.summary.push_str(&format!(
                " {skipped} finding(s) were omitted because they were already posted on this PR."
            ));
        }
        github::post_review(
            github_token,
            repo,
            *number,
            review,
            &input.commentable,
            github::expected_head_sha().as_deref(),
        )?;
        return Ok(());
    }

    if let ReviewTarget::Commit { sha, .. } = target {
        github::post_commit_review(github_token, repo, sha, review, &input.commentable)?;
    }
    Ok(())
}
