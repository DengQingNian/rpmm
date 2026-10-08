<script setup lang="ts">
import {
  NConfigProvider,
  NDialogProvider,
  NMessageProvider,
  zhCN,
  dateZhCN,
} from "naive-ui";
import DesktopShell from "./components/DesktopShell.vue";
import { sketchTheme } from "./theme";
</script>
<template>
  <NConfigProvider
    :theme-overrides="sketchTheme"
    :locale="zhCN"
    :date-locale="dateZhCN"
  >
    <NMessageProvider placement="bottom-right" :max="3"
      ><NDialogProvider><DesktopShell /></NDialogProvider
    ></NMessageProvider>
  </NConfigProvider>
</template>
