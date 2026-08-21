import claudeIcon from "@lobehub/icons-static-svg/icons/claude-color.svg";
import deepseekIcon from "@lobehub/icons-static-svg/icons/deepseek-color.svg";
import geminiIcon from "@lobehub/icons-static-svg/icons/gemini-color.svg";
import kimiIcon from "@lobehub/icons-static-svg/icons/kimi-color.svg";
import moonshotIcon from "@lobehub/icons-static-svg/icons/moonshot.svg";
import openaiIcon from "@lobehub/icons-static-svg/icons/openai.svg";
import qwenIcon from "@lobehub/icons-static-svg/icons/qwen-color.svg";
import zhipuIcon from "@lobehub/icons-static-svg/icons/zhipu-color.svg";

interface ModelIdentity {
  id: string;
  name: string;
  vendor: string;
}

export type ModelBrand = "openai" | "claude" | "deepseek" | "qwen" | "zhipu" | "kimi" | "moonshot" | "gemini";

const brandMatchers: Array<{
  brand: ModelBrand;
  colored: boolean;
  icon: string;
  terms: string[];
}> = [
  { brand: "openai", colored: false, icon: openaiIcon, terms: ["openai", "gpt-", "gpt ", "chatgpt", "o1-", "o3-", "o4-"] },
  { brand: "claude", colored: true, icon: claudeIcon, terms: ["anthropic", "claude"] },
  { brand: "deepseek", colored: true, icon: deepseekIcon, terms: ["deepseek"] },
  { brand: "qwen", colored: true, icon: qwenIcon, terms: ["qwen", "alibaba", "dashscope"] },
  { brand: "zhipu", colored: true, icon: zhipuIcon, terms: ["zhipu", "chatglm", "glm-"] },
  { brand: "kimi", colored: true, icon: kimiIcon, terms: ["kimi"] },
  { brand: "moonshot", colored: false, icon: moonshotIcon, terms: ["moonshot"] },
  { brand: "gemini", colored: true, icon: geminiIcon, terms: ["gemini", "google"] },
];

export function resolveModelIcon(model: ModelIdentity) {
  const identity = `${model.vendor} ${model.id} ${model.name}`.toLocaleLowerCase();
  return brandMatchers.find(({ terms }) => terms.some((term) => identity.includes(term))) ?? null;
}
