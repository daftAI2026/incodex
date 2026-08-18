// @ts-nocheck
"use strict";

const AUTH_URL = /accounts\.google|login|signin|oauth|authorize|auth0|okta|sso/i;
const APP_URL = /^(file:|app:|codex:)|chatgpt\.com|openai\.com/i;

function classifyWindow(snapshot) {
  if (!snapshot) return "main";
  const url = String(snapshot.url || "");
  if (AUTH_URL.test(url) || snapshot.hasParent) return "main";
  if (snapshot.alwaysOnTop && snapshot.focusable === false) return "auxiliary";
  if (snapshot.alwaysOnTop && isSmall(snapshot) && !APP_URL.test(url)) return "auxiliary";
  return "main";
}

function isSmall(snapshot) {
  const width = Number(snapshot.width) || 0;
  const height = Number(snapshot.height) || 0;
  return width > 0 && height > 0 && (width < 400 || height < 300);
}

function isAuxiliarySnapshot(snapshot) {
  return classifyWindow(snapshot) === "auxiliary";
}

export { classifyWindow, isAuxiliarySnapshot };
