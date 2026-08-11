---
title: Set Up Koharu Agent
description: Sign in with ChatGPT and choose the Codex model and reasoning level used by Koharu Agent.
---

# Set Up Koharu Agent

Koharu Agent is a project-aware Codex client built into the editor. It uses device-code authentication with your ChatGPT account and does not ask for an OpenAI API key.

## Sign in

1. Open a project and select the **Agent** panel on the right.
2. Choose **Sign in with Codex**.
3. Koharu opens the device authorization page in your browser.
4. Enter the displayed device code and approve the sign-in.
5. Return to Koharu and wait for the account and model list to load.

Only one sign-in attempt can run at a time. Cancel it before starting another.

## Configure the agent

Choose a Codex model and reasoning level from the Agent properties. Higher reasoning levels can help with long, ambiguous project edits but take more time. Koharu saves the agent configuration separately from translation-provider settings.

The Agent uses the open project's semantic state as its starting context. It does not automatically upload every page image. A page is rendered and attached only when the agent invokes its visual page-inspection tool.

## Account boundary

Agent authentication is separate from credentials under **Settings -> Providers**. Signing into ChatGPT does not configure OpenAI as the pipeline translator, and entering an OpenAI API key does not sign in Koharu Agent.

Use **Sign out** to cancel active work, clear the current agent session, and remove the Codex account authorization used by Koharu.

Continue with [Work with projects](/agent/work-with-projects/).
