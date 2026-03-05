// Post/update a sticky PR comment with tokf verify results.
// Called from the verify-comment workflow via actions/github-script.
// Receives `github`, `context`, `core`, and `artifactDir` (path to
// downloaded verify-results artifact).
module.exports = async ({ github, context, core, artifactDir }) => {
  const fs = require('fs');
  const path = require('path');

  // Read PR number from artifact
  const prNumber = parseInt(
    fs.readFileSync(path.join(artifactDir, 'pr-number.txt'), 'utf8').trim(),
    10
  );
  if (!prNumber) {
    core.info('No PR number found in artifact, skipping comment');
    return;
  }

  // Read verify outputs
  let jsonResults = [];
  try {
    jsonResults = JSON.parse(
      fs.readFileSync(path.join(artifactDir, 'verify-results.json'), 'utf8')
    );
  } catch {
    core.info('No JSON results found, skipping comment');
    return;
  }

  let stderrText = '';
  try {
    stderrText = fs.readFileSync(path.join(artifactDir, 'verify-stderr.txt'), 'utf8');
  } catch {
    // stderr file may not exist
  }

  // Detect changed filter files via the GitHub compare API (works without
  // a local checkout of the PR branch).
  let changedFilters = [];
  try {
    const { data: pr } = await github.rest.pulls.get({
      owner: context.repo.owner,
      repo: context.repo.repo,
      pull_number: prNumber,
    });
    const { data: compare } = await github.rest.repos.compareCommits({
      owner: context.repo.owner,
      repo: context.repo.repo,
      base: pr.base.sha,
      head: pr.head.sha,
    });
    changedFilters = (compare.files || [])
      .map(f => f.filename)
      .filter(f => f.startsWith('crates/tokf-cli/filters/') && f.endsWith('.toml') && !f.includes('_test/'))
      .map(f => {
        const match = f.match(/filters\/(.+)\.toml$/);
        return match ? match[1] : null;
      })
      .filter(Boolean);
  } catch (e) {
    core.warning(`Could not determine changed filters: ${e.message}`);
  }

  // Build summary data
  let totalCases = 0;
  let totalPassed = 0;
  const suiteRows = [];

  for (const suite of jsonResults) {
    const cases = suite.cases || [];
    const passed = cases.filter(c => c.passed).length;
    const failed = cases.length - passed;
    totalCases += cases.length;
    totalPassed += passed;

    suiteRows.push({
      name: suite.filter_name,
      total: cases.length,
      passed,
      failed,
      error: suite.error || null,
    });
  }

  // Parse uncovered filters from stderr
  const uncoveredFilters = [];
  const uncoveredMatch = stderrText.match(/uncovered filters[^:]*:([\s\S]*?)(?:\n\nRun|$)/);
  if (uncoveredMatch) {
    const lines = uncoveredMatch[1].trim().split('\n');
    for (const line of lines) {
      const name = line.trim();
      if (name) uncoveredFilters.push(name);
    }
  }

  // Build markdown
  const marker = '<!-- tokf-verify-report -->';
  let body = `${marker}\n## Filter Verification Report\n\n`;

  // Changed Filters section
  body += '### Changed Filters\n\n';
  if (changedFilters.length === 0) {
    body += '_No filter files changed in this PR._\n\n';
  } else {
    body += '| Filter | Status | Tests | Passed | Failed |\n';
    body += '|--------|--------|-------|--------|--------|\n';
    for (const name of changedFilters) {
      const row = suiteRows.find(r => r.name === name);
      if (row) {
        const icon = row.failed === 0 && !row.error ? ':white_check_mark:' : ':x:';
        body += `| ${name} | ${icon} | ${row.total} | ${row.passed} | ${row.failed} |\n`;
      } else {
        body += `| ${name} | :warning: | — | — | — |\n`;
      }
    }
    body += '\n';
  }

  // All Filters Summary
  body += '### All Filters Summary\n\n';
  const allPassed = totalPassed === totalCases && uncoveredFilters.length === 0;
  const summaryIcon = allPassed ? ':white_check_mark:' : ':x:';
  body += `${summaryIcon} ${totalPassed}/${totalCases} test cases passed across ${suiteRows.length} filters\n\n`;

  // Missing Test Suites
  if (uncoveredFilters.length > 0) {
    body += '### Missing Test Suites\n\n';
    body += 'The following filters have no `_test/` directory:\n\n';
    for (const name of uncoveredFilters) {
      body += `- \`${name}\`\n`;
    }
    body += '\n';
  }

  // Failures
  const failedSuites = suiteRows.filter(r => r.failed > 0 || r.error);
  if (failedSuites.length > 0) {
    body += '### Failures\n\n';
    for (const suite of failedSuites) {
      const suiteData = jsonResults.find(s => s.filter_name === suite.name);
      if (suite.error) {
        body += `<details>\n<summary>:x: ${suite.name} — error</summary>\n\n\`\`\`\n${suite.error}\n\`\`\`\n</details>\n\n`;
        continue;
      }
      const failedCases = (suiteData.cases || []).filter(c => !c.passed);
      for (const c of failedCases) {
        const failures = (c.failures || []).join('\n');
        body += `<details>\n<summary>:x: ${suite.name} / ${c.name}</summary>\n\n\`\`\`\n${failures}\n\`\`\`\n</details>\n\n`;
      }
    }
  }

  body += '---\n<sub>Generated by <code>tokf verify</code></sub>\n';

  // Find existing comment with marker (sticky pattern), paginating to
  // handle PRs with many comments.
  try {
    let allComments = [];
    let page = 1;
    while (true) {
      const { data } = await github.rest.issues.listComments({
        owner: context.repo.owner,
        repo: context.repo.repo,
        issue_number: prNumber,
        per_page: 100,
        page,
      });
      allComments = allComments.concat(data);
      if (data.length < 100) break;
      page += 1;
    }

    const existing = allComments.find(c => c.body && c.body.includes(marker));

    if (existing) {
      await github.rest.issues.updateComment({
        owner: context.repo.owner,
        repo: context.repo.repo,
        comment_id: existing.id,
        body,
      });
    } else {
      await github.rest.issues.createComment({
        owner: context.repo.owner,
        repo: context.repo.repo,
        issue_number: prNumber,
        body,
      });
    }
  } catch (error) {
    core.warning(`Failed to post verify PR comment: ${error.message || error}`);
  }
};
