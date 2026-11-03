<script setup lang="ts">
import type { Mode } from "../types";

const props = defineProps<{ modelValue: Mode }>();
const emit = defineEmits<{ "update:modelValue": [value: Mode] }>();

function choose(mode: Mode) {
  emit("update:modelValue", mode);
}

function onKey(event: KeyboardEvent) {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  event.preventDefault();
  choose(props.modelValue === "list" ? "account" : "list");
}
</script>

<template>
  <div class="mode-switch" role="radiogroup" aria-label="归档模式" @keydown="onKey">
    <button
      type="button"
      role="radio"
      :aria-checked="modelValue === 'list'"
      :class="{ on: modelValue === 'list' }"
      @click="choose('list')"
    >
      链接列表
    </button>
    <button
      type="button"
      role="radio"
      :aria-checked="modelValue === 'account'"
      :class="{ on: modelValue === 'account' }"
      @click="choose('account')"
    >
      公众号
    </button>
  </div>
</template>
