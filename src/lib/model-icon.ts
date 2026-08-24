import ai21Icon from "@lobehub/icons-static-svg/icons/ai21-brand-color.svg";
import baiduIcon from "@lobehub/icons-static-svg/icons/baidu-color.svg";
import bedrockIcon from "@lobehub/icons-static-svg/icons/bedrock-color.svg";
import cerebrasIcon from "@lobehub/icons-static-svg/icons/cerebras-color.svg";
import claudeIcon from "@lobehub/icons-static-svg/icons/claude-color.svg";
import cohereIcon from "@lobehub/icons-static-svg/icons/cohere-color.svg";
import deepseekIcon from "@lobehub/icons-static-svg/icons/deepseek-color.svg";
import doubaoIcon from "@lobehub/icons-static-svg/icons/doubao-color.svg";
import geminiIcon from "@lobehub/icons-static-svg/icons/gemini-color.svg";
import groqIcon from "@lobehub/icons-static-svg/icons/groq.svg";
import hunyuanIcon from "@lobehub/icons-static-svg/icons/hunyuan-color.svg";
import kimiIcon from "@lobehub/icons-static-svg/icons/kimi-color.svg";
import metaIcon from "@lobehub/icons-static-svg/icons/meta-color.svg";
import minimaxIcon from "@lobehub/icons-static-svg/icons/minimax-color.svg";
import mistralIcon from "@lobehub/icons-static-svg/icons/mistral-color.svg";
import moonshotIcon from "@lobehub/icons-static-svg/icons/moonshot.svg";
import nvidiaIcon from "@lobehub/icons-static-svg/icons/nvidia-color.svg";
import openaiIcon from "@lobehub/icons-static-svg/icons/openai.svg";
import perplexityIcon from "@lobehub/icons-static-svg/icons/perplexity-color.svg";
import qwenIcon from "@lobehub/icons-static-svg/icons/qwen-color.svg";
import xaiIcon from "@lobehub/icons-static-svg/icons/xai.svg";
import yiIcon from "@lobehub/icons-static-svg/icons/yi-color.svg";
import zhipuIcon from "@lobehub/icons-static-svg/icons/zhipu-color.svg";

interface ModelIdentity {
  id: string;
  name: string;
  vendor: string;
}

export type ModelBrand =
  | "openai"
  | "claude"
  | "deepseek"
  | "qwen"
  | "zhipu"
  | "kimi"
  | "moonshot"
  | "gemini"
  | "minimax"
  | "xai"
  | "mistral"
  | "meta"
  | "cohere"
  | "hunyuan"
  | "doubao"
  | "baidu"
  | "yi"
  | "bedrock"
  | "ai21"
  | "nvidia"
  | "perplexity"
  | "groq"
  | "cerebras";

interface BrandAsset {
  brand: ModelBrand;
  colored: boolean;
  icon: string;
}

const brandAssets: Record<string, BrandAsset> = {
  openai: { brand: "openai", colored: false, icon: openaiIcon },
  anthropic: { brand: "claude", colored: true, icon: claudeIcon },
  google: { brand: "gemini", colored: true, icon: geminiIcon },
  deepseek: { brand: "deepseek", colored: true, icon: deepseekIcon },
  qwen: { brand: "qwen", colored: true, icon: qwenIcon },
  moonshot: { brand: "moonshot", colored: false, icon: moonshotIcon },
  zhipu: { brand: "zhipu", colored: true, icon: zhipuIcon },
  minimax: { brand: "minimax", colored: true, icon: minimaxIcon },
  xai: { brand: "xai", colored: false, icon: xaiIcon },
  mistral: { brand: "mistral", colored: true, icon: mistralIcon },
  meta: { brand: "meta", colored: true, icon: metaIcon },
  cohere: { brand: "cohere", colored: true, icon: cohereIcon },
  tencent: { brand: "hunyuan", colored: true, icon: hunyuanIcon },
  bytedance: { brand: "doubao", colored: true, icon: doubaoIcon },
  baidu: { brand: "baidu", colored: true, icon: baiduIcon },
  "01ai": { brand: "yi", colored: true, icon: yiIcon },
  amazon: { brand: "bedrock", colored: true, icon: bedrockIcon },
  ai21: { brand: "ai21", colored: true, icon: ai21Icon },
  nvidia: { brand: "nvidia", colored: true, icon: nvidiaIcon },
  perplexity: {
    brand: "perplexity",
    colored: true,
    icon: perplexityIcon,
  },
  groq: { brand: "groq", colored: false, icon: groqIcon },
  cerebras: { brand: "cerebras", colored: true, icon: cerebrasIcon },
};

const kimiAsset: BrandAsset = {
  brand: "kimi",
  colored: true,
  icon: kimiIcon,
};

const brandMatchers: Array<{ asset: BrandAsset; terms: string[] }> = [
  { asset: kimiAsset, terms: ["kimi"] },
  {
    asset: brandAssets.openai,
    terms: ["openai", "gpt-", "gpt ", "chatgpt", "o1-", "o3-", "o4-"],
  },
  { asset: brandAssets.anthropic, terms: ["anthropic", "claude"] },
  { asset: brandAssets.google, terms: ["google", "gemini"] },
  { asset: brandAssets.deepseek, terms: ["deepseek"] },
  { asset: brandAssets.qwen, terms: ["qwen", "alibaba", "dashscope"] },
  { asset: brandAssets.moonshot, terms: ["moonshot"] },
  { asset: brandAssets.zhipu, terms: ["zhipu", "z-ai", "chatglm", "glm-"] },
  { asset: brandAssets.minimax, terms: ["minimax"] },
  { asset: brandAssets.xai, terms: ["x-ai", "xai", "grok"] },
  {
    asset: brandAssets.mistral,
    terms: ["mistral", "mixtral", "codestral", "pixtral", "magistral"],
  },
  { asset: brandAssets.meta, terms: ["meta-llama", "llama-"] },
  { asset: brandAssets.cohere, terms: ["cohere", "command-r", "command-a"] },
  { asset: brandAssets.tencent, terms: ["tencent", "hunyuan"] },
  {
    asset: brandAssets.bytedance,
    terms: ["bytedance", "doubao", "volcengine"],
  },
  { asset: brandAssets.baidu, terms: ["baidu", "qianfan", "ernie"] },
  { asset: brandAssets["01ai"], terms: ["01-ai", "01ai", "yi-"] },
  { asset: brandAssets.amazon, terms: ["amazon", "bedrock", "nova-"] },
  { asset: brandAssets.ai21, terms: ["ai21", "jamba"] },
  { asset: brandAssets.nvidia, terms: ["nvidia", "nemotron"] },
  { asset: brandAssets.perplexity, terms: ["perplexity", "sonar-"] },
  { asset: brandAssets.groq, terms: ["groq"] },
  { asset: brandAssets.cerebras, terms: ["cerebras"] },
];

export function resolveModelIcon(model: ModelIdentity) {
  const identity =
    `${model.vendor} ${model.id} ${model.name}`.toLocaleLowerCase();
  return (
    brandMatchers.find(({ terms }) =>
      terms.some((term) => identity.includes(term)),
    )?.asset ?? null
  );
}
