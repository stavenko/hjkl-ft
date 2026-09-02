#!/usr/bin/env bash
# Запрет на переписывание чужой истории. Ставится как PreToolUse-хук на Bash.
#
# Два правила, оба заданы человеком:
#   1. force-push в main/master — НИКОГДА;
#   2. переписывать коммиты можно ТОЛЬКО в текущей рабочей ветке.
#
# Почему хук, а не одни permissions.deny: правила там сопоставляются префиксом
# строки, а force-push пишется десятком способов — `-f`, `--force`,
# `--force-with-lease`, `+main` в refspec, `--mirror`. Префиксный список ловит
# первое попавшееся написание и пропускает остальные. Здесь команда разбирается
# целиком, поэтому обойти её опечаткой в порядке флагов нельзя.
#
# Разрешено: обычный push куда угодно и force-push в СВОЮ текущую ветку (ровно
# то, чем переразбивают коммиты перед ревью).

set -uo pipefail

payload=$(cat)
cmd=$(printf '%s' "$payload" | jq -r '.tool_input.command // ""' 2>/dev/null || echo "")

# Не про git push — пропускаем молча.
[[ "$cmd" == *"git"*"push"* ]] || exit 0

# Кавычки СНИМАЮТСЯ до разбора. Иначе `bash -c "git push --force origin main"`
# и любая обёртка через eval прошли бы мимо: слово `git` в них склеено с
# кавычкой и не опознаётся. Платой идёт ложное срабатывание на команде, которая
# такую строку всего лишь печатает, — цена приемлемая: напечатать можно иначе, а
# пропущенный force-push в main не отыграть.
cmd="${cmd//\"/ }"
cmd="${cmd//\'/ }"

deny() {
    jq -nc --arg r "$1" '{
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason: $r
      }
    }'
    exit 0
}

current_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")

# Команда может быть составной (`a && git push …`). Разбираем КАЖДЫЙ git push,
# который в ней встретился: запрещённым делает и один из них.
# shellcheck disable=SC2206
tokens=($cmd)
i=0
n=${#tokens[@]}
while (( i < n )); do
    # Ищем начало `git … push`. Между ними могут стоять глобальные ключи git
    # (`git -C dir push`), поэтому push ищем чуть дальше слова git.
    if [[ "${tokens[i]}" != "git" && "${tokens[i]}" != *"/git" ]]; then
        ((i++)); continue
    fi
    j=$((i + 1))
    while (( j < n )) && [[ "${tokens[j]}" == -* || "${tokens[j]}" == "-C" ]]; do
        # `-C <путь>` съедает ещё одно слово.
        [[ "${tokens[j]}" == "-C" ]] && ((j++))
        ((j++))
    done
    if (( j >= n )) || [[ "${tokens[j]}" != "push" ]]; then
        i=$((i + 1)); continue
    fi

    forced=0
    mirror=0
    refspecs=()
    remote_seen=0
    k=$((j + 1))
    while (( k < n )); do
        t="${tokens[k]}"
        # Конец этой команды в составной строке.
        [[ "$t" == "&&" || "$t" == "||" || "$t" == ";" || "$t" == "|" ]] && break
        case "$t" in
            --mirror)
                mirror=1; forced=1 ;;
            --force|--force-with-lease|--force-if-includes)
                forced=1 ;;
            --force-with-lease=*|--force-if-includes=*)
                forced=1 ;;
            -f|-*f) # -f как отдельный ключ и в связке вида -uf
                [[ "$t" == -[!-]* && "$t" == *f* ]] && forced=1 ;;
            -*) : ;; # прочие ключи не трогаем
            *)
                if (( remote_seen == 0 )); then
                    remote_seen=1        # имя удалённого репозитория
                else
                    refspecs+=("$t")
                fi ;;
        esac
        ((k++))
    done

    if (( mirror == 1 )); then
        deny "ЗАПРЕЩЕНО: push с ключом --mirror перезаписывает ВСЕ ветки удалённого репозитория, включая main. Отправьте свою ветку поимённо."
    fi

    if (( forced == 1 )); then
        # Ветки назначения. Без явного refspec push идёт в текущую ветку.
        targets=()
        if (( ${#refspecs[@]} == 0 )); then
            targets=("$current_branch")
        else
            for r in "${refspecs[@]}"; do
                r="${r#+}"                       # `+main` — тоже force
                targets+=("${r##*:}")            # из `src:dst` берём dst
            done
        fi
        for t in "${targets[@]}"; do
            t="${t#refs/heads/}"
            case "$t" in
                main|master)
                    deny "ЗАПРЕЩЕНО: force-push в «$t». Правило проекта: историю main/master не переписывают никогда. Влейте ветку обычным merge/PR." ;;
            esac
            if [[ -n "$current_branch" && "$current_branch" != "HEAD" && "$t" != "$current_branch" ]]; then
                deny "ЗАПРЕЩЕНО: force-push в «$t», а текущая ветка — «$current_branch». Правило проекта: переписывать коммиты можно только в своей рабочей ветке."
            fi
        done
    fi

    # refspec с `+` форсит и без единого ключа: `git push origin +main`.
    for r in "${refspecs[@]}"; do
        [[ "$r" == +* ]] || continue
        t="${r#+}"; t="${t##*:}"; t="${t#refs/heads/}"
        case "$t" in
            main|master)
                deny "ЗАПРЕЩЕНО: refspec «$r» форсит запись в «$t». Историю main/master не переписывают." ;;
        esac
        if [[ -n "$current_branch" && "$current_branch" != "HEAD" && "$t" != "$current_branch" ]]; then
            deny "ЗАПРЕЩЕНО: refspec «$r» форсит запись в «$t», а текущая ветка — «$current_branch»."
        fi
    done

    i=$((k > i ? k : i + 1))
done

exit 0
