---
title: Translate
description: Select text, press a shortcut, and read the translation next to it. Works in editors, the terminal and Markdown views, with DeepL, Google Cloud Translation, an OpenAI-compatible API or Google's free endpoint.
---

# Translate

Select some text, press the shortcut of a language pair, and the translation appears in a small popover next to the selection. Each language pair has its own shortcut, for example one for Vietnamese to English and one for English to Vietnamese. Zode does not translate anything itself: it sends the text to the service you choose.

## Quick start

1. Open Settings ({#kb zode::OpenSettings}) and go to **Translate**.
2. Pick a **Provider**, then press **Set API Key…** and paste your key. A local server such as Ollama, or Google Translate (free, no key), needs no key.
3. Select some text in an editor and press Cmd-Shift-1 on a Mac (Ctrl-Shift-1 on Linux and Windows) to translate from Vietnamese to English, or Cmd-Shift-2 (Ctrl-Shift-2) for English to Vietnamese.

## Default shortcuts

| Language pair        | macOS   | Linux    | Windows        | On a US keyboard                     |
| -------------------- | ------- | -------- | -------------- | ------------------------------------ |
| Vietnamese → English | `cmd-!` | `ctrl-!` | `ctrl-shift-1` | Cmd (Mac) or Ctrl (others), Shift, 1 |
| English → Vietnamese | `cmd-@` | `ctrl-@` | `ctrl-shift-2` | Cmd (Mac) or Ctrl (others), Shift, 2 |

On macOS and Linux the keymap names the character that Shift-1 and Shift-2 type on a US layout, `!` and `@`. On a layout where Shift-1 or Shift-2 types something else, the defaults may fire on a different key or not at all. Pick keys that suit your keyboard with **Edit Shortcuts…** (see [Language pairs and shortcuts](#language-pairs-and-shortcuts)).

On Linux, the Emacs keymap already uses Ctrl-Shift-2 (set mark) in editors, and the Sublime Text keymap uses Ctrl-Shift-1 and Ctrl-Shift-2 to move tabs between panes. With one of those base keymaps, reassign the pairs with **Edit Shortcuts…**.

## Where it works

The same shortcut works in three places. Each language pair is one shortcut that covers all of them.

- **Editors and input fields**: files open in an editor (code files included) and text inputs such as the search bar, dialogs and the Settings window. The text of the newest selection is sent.
- **The terminal**: the selected text or, with nothing selected, the text the program in the terminal has just copied, such as a selection made in Claude Code's full-screen mode. That copy is used until you type or paste, for up to 30 seconds, and only once. Because the program chose that text, not you, the popover first shows its first line and length and sends nothing until you choose **Translate** or press Enter. Otherwise the key goes to the program running in the terminal, so `vim` or `tmux` still receive it. Text copied from other apps is never translated.
- **Markdown views**: the Markdown preview and the panels that show rendered Markdown, such as the agent panel.

Pressing the shortcut with nothing selected does nothing in an editor or a Markdown view.

## The popover

The popover opens below the end of the selection in an editor, and near the mouse pointer in the terminal and in Markdown views. Its header names the provider and the direction, for example _DeepL · Vietnamese → English_. While it waits it shows _Translating…_.

- **Copy** copies the translation.
- **Swap direction** translates the same text the other way.
- **Replace** writes the translation over the original text (editors only; see below).

The popover closes when you press Escape, click outside it, change the selection or scroll the editor. Pressing a translate shortcut again replaces it with a new one and cancels the request that was still running.

If the provider is not set up yet, for example the key is missing or rejected, the popover says so and offers **Open Translate settings**. Other failures, such as a timeout or a network error, are shown in the popover as the provider reported them. Zode never switches to another provider on its own.

## Replace and undo

**Replace** appears only when the text came from an editor or input field you can type in, including code files. It does not appear for read-only editors, the terminal or Markdown views.

The translation is written only if the original text is still exactly as it was when you pressed the shortcut. If you edited that text in the meantime, the button is disabled and the popover says _The text changed since it was translated._ Replace is a single edit: one undo ({#kb editor::Undo}) brings the original text back. The selection's leading and trailing spaces and line breaks are kept, and in a single-line field line breaks in the translation become spaces.

## Providers and API keys

Choose the provider under **Settings → Translate → Provider**. The key of each provider is kept separately, so switching providers does not lose a key.

| Provider                        | Key                  | Notes                                                                                     |
| ------------------------------- | -------------------- | ----------------------------------------------------------------------------------------- |
| DeepL                           | DeepL API key        | Free keys end in `:fx` and are sent to DeepL's free endpoint automatically.               |
| Google Cloud Translation        | Google Cloud API key | Uses the Cloud Translation API (Basic, v2).                                               |
| OpenAI-compatible API           | Optional             | Any service with an OpenAI-style `/chat/completions` endpoint: OpenAI, Ollama, LM Studio. |
| Google Translate (free, no key) | None                 | Unofficial free endpoint. May be rate limited or stop working. Never the default.         |

### DeepL

Sign up for the DeepL API (the API plan, not DeepL Translator). The free API plan allows 500,000 characters a month. Copy the authentication key from your DeepL account; a free key ends in `:fx`.

DeepL added Vietnamese in 2025, at first for the Pro API only. If your plan does not offer it, the popover says _DeepL cannot translate from "vi" to "en"._ In that case use another provider for Vietnamese.

### Google Cloud Translation

In the Google Cloud console, create or pick a project, enable the **Cloud Translation API**, and create an API key under **APIs & Services → Credentials**. Restricting the key to the Cloud Translation API is a good idea.

### OpenAI-compatible API

Set **Base URL** to the address of the API up to and including the version, and **Model** to the model that should translate. The model has no default, so translation fails until you set it. Plain `http://` is accepted only for your own computer (`localhost`, `127.x.x.x`, `[::1]`); the address cannot carry a user name, a password, a `?query` or a `#fragment`.

- **Ollama**: the default Base URL, `http://localhost:11434/v1`, with a model you have pulled, for example `llama3.1`. No key.
- **LM Studio**: start its local server and use `http://localhost:1234/v1` with the model you loaded. No key.
- **OpenAI** or another hosted service: its `https://` address, such as `https://api.openai.com/v1`, a model such as `gpt-4o-mini`, and its API key. The key is saved for that server: if you later point Base URL at another server, Zode asks for the key again instead of sending it there.

Plain `http://` is only accepted for `localhost` and `127.0.0.1`; every other address must use `https://`.

### Google Translate (free, no key)

This provider needs no key and costs nothing. It sends the text to the endpoint behind Google's free web translation widgets (`translate.googleapis.com/translate_a/single`), not to the Cloud Translation API. That endpoint is unofficial: Google does not document it or support it for apps like Zode.

- Google may slow it down or block it at any time, most often after many requests in a short time. The popover then says _Google is limiting free requests right now_. Wait a while, or switch to a provider with a key.
- It may stop working altogether if Google changes or closes the endpoint. Zode does not switch to another provider on its own.
- Using it is subject to Google's terms of service. For steady or heavy use, Google Cloud Translation with your own key is the supported way.

It is never the default: pick it under **Provider** only if you want it.

### Setting the key

Press **Set API Key…** on the Translate page, or run `translate: set api key` from the command palette. The dialog is for the provider that is selected now.

- **Save** stores the key in the system keychain. Spaces and line breaks around a pasted key are removed.
- **Remove** deletes the stored key.
- **Test** translates a short word to check that the key and settings work.

For Google Translate (free, no key) the dialog has no key field, **Save** or **Remove**: it says that no key is needed and keeps **Test**.

Keys never go into `settings.json`. Do not put them there.

## Language pairs and shortcuts

Press **Edit Shortcuts…** on the Translate page, or run `translate: edit shortcuts` from the command palette. The dialog lists every language pair in your keymap with its shortcut.

- **Change a shortcut**: click the shortcut of a pair, press the new keys and press **Save**.
- **Delete a pair**: press the trash button on its row.
- **Add a pair**: under **Add Language Pair**, pick the two languages, record a shortcut and press **Add**. The two languages must differ.

If the keys are already used by something else, the dialog names it and does not save. Press **Save** or **Add** again to use the keys anyway. Changes are written to your keymap file, so they also show up in the Keymap Editor. Each pair is saved for editors, the terminal and Markdown views together, and the dialog edits those entries as one.

You can also write a pair in your keymap by hand. The action takes two language codes, such as `en`, `vi`, `pt-BR` or `zh-Hans`:

```json [keymap]
[
  {
    "context": "Editor",
    "bindings": {
      "alt-shift-3": [
        "translate::TranslateSelection",
        { "from": "ja", "to": "en" }
      ]
    }
  }
]
```

A binding in `Editor` only works in editors. To use it in the terminal and Markdown views too, add the same binding under the contexts `Terminal` and `Markdown`, or use the dialog, which does this for you.

## Privacy

- Nothing is sent until you press a translate shortcut. Then only the selected text is sent, and only to the provider you chose.
- DeepL, Google and hosted OpenAI-compatible services receive that text under their own terms. To keep the text on your computer, use a local server such as Ollama or LM Studio.
- Google Translate (free, no key) sends the selected text to Google without any account or key, under Google's terms. Do not use it for text you would not paste into Google Translate yourself.
- API keys are kept in the system keychain, never in `settings.json`.

## Limits

- One selection at a time: with several cursors, only the newest selection is translated.
- The shortcuts work inside Zode only; there is no system-wide shortcut.
- Each shortcut has a fixed direction. Zode does not detect the language of the text.
- A selection longer than **Max Characters** (2,000 by default) is not sent and not cut; the popover shows a warning instead.
- Replace works in editors only, not in the terminal or Markdown views.

## Settings

| Setting                                | Default                       | What it does                                                            |
| -------------------------------------- | ----------------------------- | ----------------------------------------------------------------------- |
| `translate.provider`                   | `"deepl"`                     | `"deepl"`, `"google"`, `"openai_compatible"` or `"google_free"`.        |
| `translate.openai_compatible.base_url` | `"http://localhost:11434/v1"` | The API address for the OpenAI-compatible provider.                     |
| `translate.openai_compatible.model`    | `""`                          | The model that translates. Required for the OpenAI-compatible provider. |
| `translate.max_characters`             | `2000`                        | The longest selection that is sent.                                     |
| `translate.timeout_seconds`            | `20`                          | How long to wait for a translation before giving up.                    |

The API key and the shortcuts are not settings: the key is in the keychain and the shortcuts are in your keymap. See [All Settings](./reference/all-settings.md#translate) for the full section.
