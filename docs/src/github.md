---
title: GitHub
description: Work with the issues, epics, projects and pull requests of the repository you have open, in a GitHub tab that talks to GitHub straight from your machine.
---

# GitHub

The GitHub tab shows the issues, pull requests and projects of the repository your project's git remote points at. It talks to GitHub straight from your computer with your own account; nothing goes through a zodekit server, and it costs nothing.

## Opening the tab

Click the GitHub icon in the sidebar rail, or run `github: toggle` from the command palette. The tab opens in the middle of the window like a file. Pressing the button again while the tab is in front steps back to the tab you were reading; the GitHub tab stays open and signed in.

The tab is never restored when Zode starts, so opening Zode sends nothing to GitHub. Zode looks for a token only when you open the tab.

## Signing in

There are two ways in, and the tab says which one it is using next to your `@login`.

- **The GitHub CLI.** If `gh` is installed and signed in, Zode asks it for its token (`gh auth token --hostname github.com`) each time the tab opens or reloads. Zode does not store that token. When `gh` holds several accounts, Zode uses the active one; change it with `gh auth switch` and press **Reload**. A `GH_TOKEN` or `GITHUB_TOKEN` variable in Zode's environment wins over `gh`'s login, and the tab says so.
- **Sign in with GitHub.** Zode asks GitHub for a short code; you enter it at `github.com/login/device` and approve. This button only appears in builds that carry zodekit's GitHub app; otherwise use the GitHub CLI.

Sign in with GitHub asks for three scopes:

| Scope      | Why                                                                          |
| ---------- | ---------------------------------------------------------------------------- |
| `repo`     | Read and change issues and pull requests, including in private repositories. |
| `read:org` | Read your organization and team memberships.                                 |
| `project`  | Read and change GitHub Projects.                                             |

The token stays on this machine. Zode keeps it in the system keychain and sends it only to `api.github.com`; it never reaches zodekit's server, and your Zode account does not need to be a GitHub one. Development builds keep it in memory only, so you sign in again after each restart.

**Disconnect** (next to your login, or **GitHub Account** on the Panels page of Settings) forgets the token Zode signed in with and opens GitHub's page for revoking Zode's access. Zode cannot revoke a token by itself, so revoke the access there to end it for good. A token from the GitHub CLI is left alone; sign out of it with `gh auth logout`. The same page in Settings shows whether you are signed in, and **Sign in with GitHub** there opens the tab and starts signing in.

If GitHub stops accepting the token, the tab says so and waits for you. It never falls back to another token by itself, since that could quietly act as a different account.

### Projects need the `project` scope

The GitHub CLI's usual login does not include it. Without it the Projects section explains what is missing and sends nothing. To add it, run:

```sh
gh auth refresh -s project
```

Then press **Check again**. Signing in with GitHub asks for the scope from the start.

### Organizations with restrictions

- **OAuth App access restrictions.** An organization can block apps its owners have not approved. The tab names the organization and offers **Request access**; an owner then approves the app on GitHub.
- **SAML single sign-on.** The token must be authorized for the organization. The tab offers **Authorize on GitHub**, which opens GitHub's page for it; reload once you are done.

## Picking the repository

Zode reads the `origin` and `upstream` remotes of each git repository in the project and offers every repository on `github.com` it finds, labelled with its remote and whether it is public or private. It never picks upstream over your fork on its own; the first one found is selected, and you switch with the buttons at the top.

- Remotes through an SSH host alias, such as `git@github.com-work:owner/repo`, are not recognised. Use `git@github.com:` or an `https://github.com/` address.
- Only `github.com` is supported, not GitHub Enterprise Server.
- In a remote project, the remotes are read on the remote machine, but `gh`, the keychain and every request to GitHub stay on this machine. On Windows the same holds for WSL: a `gh` signed in only inside a WSL distribution is not found, so sign in with `gh` on Windows or use Sign in with GitHub.
- In a project shared by a collaborator, nothing changes the host's files: checking out a pull request is turned off.

## Issues

The Issues section lists the repository's issues. Each row shows a state icon (open, closed as completed, or closed as not planned, each with its own shape), the number, title and labels, the assignees' profile pictures, the comment count and when the issue last changed. Pull requests are left out.

- **Filters**: Open, Closed or All; Assigned to me, Created by me or Mentioning me; a label; a milestone; and an issue type when the repository has types.
- **Search**: typing in the field narrows the issues already loaded. Press Enter to search the whole repository on GitHub. Search has a tighter rate limit, so it only runs on Enter.
- **Load more** fetches the next page; nothing loads on its own as you scroll.

Click an issue to open it in its own tab: title, state, author, labels, assignees, milestone, description and comments, with **Open on GitHub** and **Copy link**.

### Changing issues

Every change comes from a button you press; nothing is written in the background.

- **New issue** opens a dialog with a title, a description in Markdown, labels, assignees, a milestone and, when the repository has issue types, a type. **Create issue** stays off until there is a title, and ⌘↩ (Ctrl-↩ on Linux and Windows) creates the issue from the keyboard. The dialog always shows the account and the repository it will create the issue in, and whether that repository is public or private; if creating fails, everything you entered stays.
- In the issue tab, **Edit** changes the title and description. If someone changed the issue on GitHub after you started editing, Zode says so instead of overwriting it, and keeps your text. The description, here and in **New issue**, and the comment boxes of issues and pull requests have **Write** and **Preview** tabs and a toolbar for headings, bold, italic, quotes, code, links and bulleted, numbered and task lists; a toolbar button applied again takes its formatting off. While typing, ⌘B, ⌘I and ⌘E (Ctrl-B, Ctrl-I and Ctrl-E on Linux and Windows) make the selection bold, italic or code.
- **Close** as completed, not planned or duplicate; **Reopen**.
- Add and remove labels and assignees one at a time (**Assign me** included), and set or clear the milestone. To assign someone, type part of their login or name in the people search, here and in the New issue dialog: once you pause typing, Zode asks GitHub for the people who can be assigned in the repository (the first 20 when the field is empty) and lists each with picture, name and `@login`. ↑ and ↓ move, Enter picks, Esc closes the list, and picking someone already assigned unassigns them. Only that one label or person is sent, so a change someone else made at the same time is kept.
- **Comment** sends a comment; ⌘↩ (Ctrl-↩ on Linux and Windows) sends it from the keyboard.
- Without permission to triage, GitHub ignores labels, assignees and milestones without an error. Zode reads the issue back and tells you what was not applied.

To change several issues at once, tick them in the list and choose **Close…** or **Add label…**. A dialog lists every issue first and asks you to confirm; then a progress bar runs, **Cancel** stops before the next issue, and each issue reports whether it worked.

## Epics: sub-issues and issue types

An issue with sub-issues shows its progress, such as _2/5 done_, and opens into its children in the list. In the issue tab, the **Sub-issues** part lists them and lets you add an existing issue, create a new one as a child, move one up or down, or remove one after confirming. **Parent** changes or clears the issue's parent.

GitHub allows at most 100 sub-issues under one parent and 8 levels of nesting; past that, Zode shows GitHub's refusal. The **Type** picker and the type filter appear only when GitHub reports issue types for the repository.

## Projects

The Projects section shows the GitHub Projects linked to the repository. Pick a project and one of its views:

- A **board** view has a column per option of the view's grouping field (Status by default), plus a column for items with no value. Each card shows whether the item is an open or closed issue, a pull request or a draft, its repository and number, title, labels, the assignees' pictures and logins, and the values of the other fields the view shows, such as Priority or an iteration.
- A **table** view shows the view's fields, sorted the way the view sorts them. Roadmap views are shown as a table. Assignees show their pictures and logins, **Linked pull requests** shows how many pull requests are linked (their numbers on hover), and **Sub-issues progress** shows how many sub-issues are done, such as 1/3; cards on a board show the same.

Zode loads the first 500 items of a project and says when there are more; **Load 500 more** fetches the next ones. GitHub does not apply a view's filter for other apps, so Zode shows the filter as text and lists every item. Items you are not allowed to see appear as _Hidden item_.

### Changing a project

- **Move** a card to another column through its right-click menu, by dragging it, or from the keyboard: select a card with ↑ and ↓, then press ⌘↩ (Ctrl-↩ on Linux and Windows) to open the menu. The card moves at once and goes back, with a message, if GitHub refuses.
- **Edit fields** in the table: text, number, date, single select and iteration. Enter saves, Escape cancels. Assignees, labels and milestone belong to the issue, so change them in the issue tab.
- **Add** an existing issue or pull request of the repository, and optionally set its Status.
- **Remove from project…** asks first. It does not delete the issue or pull request; a draft issue lives only in the project and is deleted with it.

Changing a project needs the `project` scope, not just `read:project`; the section says so when the token lacks it.

### Create a project

**New project**, next to the project picker (or **Create project** when the repository has none yet), opens a dialog. Pick the owner: **@me** (your account) or, when an organization owns the repository, that organization. Type a title (up to 256 characters) and choose **Create project**. The project is linked to the repository and picked in the section; GitHub adds a Status field with Todo, In progress and Done. When GitHub refuses (for example, an organization that does not let members create projects), the dialog shows GitHub's message and keeps the title. Creating a project needs the `project` scope as well.

## Pull requests

The Pull requests section lists pull requests with their checks (passed, failed or running), review state, branches and a _Draft_ label. Filter by Open, Closed, Merged or All, and by Created by me or Review requested. Those last two narrow only the pages already loaded; press **Load more** to look further back.

A pull request opens in its own tab with its description, comments, checks with links to their runs, reviews, and changed files with lines added and removed. You can comment from there.

### Checking out a pull request

**Check out** fetches the pull request's branch from the git remote that points at the selected repository and switches to it, in a local branch named `pr-<number>-<branch>` by default. You can edit the name; names git would refuse, names starting with `-`, and names with hidden characters are rejected. If the branch already exists, Zode asks whether to switch to it, as it is, or use another name. With uncommitted changes, Zode stops and asks you to commit or stash them first.

A pull request from a **fork** is fetched from `refs/pull/<number>/head`. Before that, Zode warns you who wrote it and where it comes from: once checked out, the code runs in a trusted worktree, where its project settings, tasks, build scripts and language servers can run without asking. Check it out only if you trust it. The warning appears every time, and Zode runs nothing for you afterwards.

### Merging

**Merge…** opens a dialog that shows your account, the pull request, the base branch and only the merge methods the repository allows. The merge is sent with the head commit you saw; if new commits were pushed meanwhile, nothing is merged and the pull request is reloaded.

- Zode never merges past branch protection, even for administrators. A pull request that is blocked, behind, conflicting or a draft says why.
- When GitHub is still working out whether a pull request can be merged, the tab asks you to reload in a moment.
- Repositories that use a merge queue are merged on GitHub: the tab offers **Merge on GitHub**.
- **Delete `<branch>` after merging** is offered only for a branch in the same repository that is not the default branch, not protected, and still at the commit that was merged. When the repository deletes merged branches itself, the dialog says so instead.

## Safety

Issue and pull request text is written by other people, so Zode draws it with care:

- Markdown is shown without HTML, and images are never downloaded; their alt text is shown instead. The only pictures the tab loads are profile pictures from `https://avatars.githubusercontent.com/`; for any other address, or a picture that fails to load, it shows the person's initials.
- Only `http` and `https` links work. A link whose text shows one address but leads to another names the real host and asks before opening. A link opens inside Zode only when it is exactly a `github.com` issue or pull request.
- Hidden characters, such as right-to-left overrides and zero-width spaces, are shown as `<U+…>` in titles, labels, names and branch names.

Every write comes from a button you press, and merging, bulk changes and removals ask first. Writes go out one at a time.

## Rate limits and reloading

Zode does not poll. It reads when you open the tab or a section, and again when you press **Reload**. An issue list that has not changed is answered from GitHub's cache and does not count against your rate limit. The tab header shows how many REST and GraphQL requests you have left and when the limit resets. If you hit the limit, the tab says so, and writes wait before they continue.

## Limitations

- Only `github.com`; GitHub Enterprise Server is not supported.
- No code review inside the editor: read the changed files in the pull request tab and review on GitHub.
- Issue templates are not offered yet; **New issue** starts from an empty form.
- Created by me and Review requested on pull requests filter only the pages loaded so far.
- No notifications and no GitHub Actions.

## Settings

| Setting               | Default | What it does                                 |
| --------------------- | ------- | -------------------------------------------- |
| `github_panel.button` | `true`  | Shows the GitHub button in the sidebar rail. |

See [All Settings](./reference/all-settings.md) for how to change it.
