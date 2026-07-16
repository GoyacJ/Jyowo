import { describe, expect, it } from 'vitest'

import { createAppI18n } from './i18n'
import { APP_LOCALES, DEFAULT_APP_LOCALE, FALLBACK_APP_LOCALE, isAppLocale } from './locales'
import { getResourceKeyPaths, resources } from './resources'

describe('i18n', () => {
  it('defines the supported locales and validates unknown values', () => {
    expect(APP_LOCALES).toEqual(['zh-CN', 'en-US'])
    expect(DEFAULT_APP_LOCALE).toBe('zh-CN')
    expect(FALLBACK_APP_LOCALE).toBe('en-US')
    expect(isAppLocale('zh-CN')).toBe(true)
    expect(isAppLocale('en-US')).toBe(true)
    expect(isAppLocale('fr-FR')).toBe(false)
  })

  it('keeps every locale resource file on the same key paths', () => {
    const englishPaths = getResourceKeyPaths(resources['en-US'])

    expect(getResourceKeyPaths(resources['zh-CN'])).toEqual(englishPaths)
  })

  it('uses Chinese by default and falls back to English before returning keys', () => {
    const i18n = createAppI18n()

    expect(i18n.language).toBe('zh-CN')
    expect(i18n.t('settings:language.title')).toBe('语言')
    i18n.addResource(
      'en-US',
      'settings',
      'language.englishOnlyFallback',
      'Fallback-only English text',
    )
    expect(i18n.t('settings:language.englishOnlyFallback')).toBe('Fallback-only English text')
    expect(i18n.t('settings:language.missingKey')).toBe('language.missingKey')
  })

  it('uses conversation terminology for the task sidebar', () => {
    const i18n = createAppI18n()

    expect(i18n.t('shell:actions.newConversation')).toBe('新建会话')
    expect(i18n.t('shell:sections.pinned')).toBe('置顶')
    expect(i18n.t('shell:sections.projects')).toBe('项目')
    expect(i18n.t('shell:sections.conversations')).toBe('会话')
    expect(i18n.t('shell:sidebar.emptyTitle')).toBe('选择一个会话')
    expect(i18n.t('conversation:composer.inputLabel')).toBe('消息输入框')

    void i18n.changeLanguage('en-US')
    expect(i18n.t('shell:actions.newConversation')).toBe('New conversation')
    expect(i18n.t('shell:sidebar.emptyTitle')).toBe('Choose a conversation')
    expect(i18n.t('conversation:composer.inputLabel')).toBe('Message input')
  })
})
