use std::collections::{BTreeMap, BTreeSet};

use koharu_scene::EntityId;

use crate::Stage;

#[derive(Clone, Copy, Eq, PartialEq)]
enum WorkState {
    Pending,
    Batched,
    Running,
    Finished,
}

struct StageWork {
    stage: Stage,
    state: WorkState,
}

struct PageWork {
    page: EntityId,
    stages: Vec<StageWork>,
}

impl PageWork {
    fn started(&self) -> bool {
        self.stages
            .iter()
            .any(|work| matches!(work.state, WorkState::Running | WorkState::Finished))
    }

    fn finished(&self) -> bool {
        self.stages
            .iter()
            .all(|work| work.state == WorkState::Finished)
    }

    fn ready(&self, index: usize) -> bool {
        let Some(prerequisite) = prerequisite(self.stages[index].stage) else {
            return true;
        };
        self.stages
            .iter()
            .find(|work| work.stage == prerequisite)
            .is_none_or(|work| work.state == WorkState::Finished)
    }
}

pub(crate) struct Scheduler {
    pages: Vec<PageWork>,
    page_index: BTreeMap<EntityId, usize>,
    page_window: usize,
    active_pages: usize,
    head: usize,
    total: usize,
    translation_batch_pages: usize,
}

pub(crate) struct ScheduledWork {
    pub(crate) page: EntityId,
    pub(crate) stage: Stage,
    pub(crate) batch_pages: Vec<EntityId>,
}

impl Scheduler {
    pub(crate) fn new(pages: &[EntityId], stages: &[Stage]) -> Self {
        let pages = pages
            .iter()
            .map(|page| PageWork {
                page: *page,
                stages: stages
                    .iter()
                    .map(|stage| StageWork {
                        stage: *stage,
                        state: WorkState::Pending,
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        let total = pages.len().saturating_mul(stages.len());
        Self {
            page_index: pages
                .iter()
                .enumerate()
                .map(|(index, page)| (page.page, index))
                .collect(),
            pages,
            page_window: stages.len().max(1),
            active_pages: 0,
            head: 0,
            total,
            translation_batch_pages: 1,
        }
    }

    pub(crate) fn with_translation_batch_pages(mut self, pages: usize) -> Self {
        self.translation_batch_pages = pages.max(1);
        self.page_window = self.page_window.max(self.translation_batch_pages);
        self
    }

    pub(crate) fn total(&self) -> usize {
        self.total
    }

    #[cfg(test)]
    pub(crate) fn start_next(
        &mut self,
        busy_stages: &BTreeSet<Stage>,
    ) -> Option<(EntityId, Stage)> {
        for page_index in self.head..self.pages.len() {
            let started = self.pages[page_index].started();
            if !started && self.active_pages >= self.page_window {
                break;
            }
            let stage_index =
                self.pages[page_index]
                    .stages
                    .iter()
                    .enumerate()
                    .find_map(|(index, work)| {
                        (work.state == WorkState::Pending
                            && !busy_stages.contains(&work.stage)
                            && self.pages[page_index].ready(index))
                        .then_some(index)
                    });
            let Some(stage_index) = stage_index else {
                continue;
            };
            if !started {
                self.active_pages += 1;
            }
            let page = &mut self.pages[page_index];
            let work = &mut page.stages[stage_index];
            work.state = WorkState::Running;
            return Some((page.page, work.stage));
        }
        None
    }

    pub(crate) fn start_next_batch(
        &mut self,
        busy_stages: &BTreeSet<Stage>,
    ) -> Option<ScheduledWork> {
        for page_index in self.head..self.pages.len() {
            let started = self.pages[page_index].started();
            if !started && self.active_pages >= self.page_window {
                break;
            }
            for stage_index in 0..self.pages[page_index].stages.len() {
                let work = &self.pages[page_index].stages[stage_index];
                if !matches!(work.state, WorkState::Pending | WorkState::Batched)
                    || busy_stages.contains(&work.stage)
                    || !self.pages[page_index].ready(stage_index)
                {
                    continue;
                }
                let stage = work.stage;
                let batch_pages = if work.state == WorkState::Batched {
                    vec![self.pages[page_index].page]
                } else if stage == Stage::Translation && self.translation_batch_pages > 1 {
                    let pages = self.ready_translation_batch(page_index);
                    if pages.len() < self.translation_batch_pages
                        && self.translation_batch_can_grow(page_index + pages.len())
                    {
                        continue;
                    }
                    pages
                } else {
                    vec![self.pages[page_index].page]
                };
                if !started {
                    self.active_pages += 1;
                }
                self.pages[page_index].stages[stage_index].state = WorkState::Running;
                if batch_pages.len() > 1 {
                    for page in &batch_pages[1..] {
                        let index = self.page_index[page];
                        if let Some(work) = self.pages[index]
                            .stages
                            .iter_mut()
                            .find(|work| work.stage == Stage::Translation)
                        {
                            work.state = WorkState::Batched;
                        }
                    }
                }
                return Some(ScheduledWork {
                    page: self.pages[page_index].page,
                    stage,
                    batch_pages,
                });
            }
        }
        None
    }

    fn ready_translation_batch(&self, start: usize) -> Vec<EntityId> {
        let mut pages = Vec::new();
        for page in &self.pages[start..] {
            let Some((index, work)) = page
                .stages
                .iter()
                .enumerate()
                .find(|(_, work)| work.stage == Stage::Translation)
            else {
                break;
            };
            if work.state != WorkState::Pending || !page.ready(index) {
                break;
            }
            pages.push(page.page);
            if pages.len() == self.translation_batch_pages {
                break;
            }
        }
        pages
    }

    fn translation_batch_can_grow(&self, start: usize) -> bool {
        self.pages[start..].iter().any(|page| {
            page.stages
                .iter()
                .enumerate()
                .find(|(_, work)| work.stage == Stage::Translation)
                .is_some_and(|(index, work)| work.state == WorkState::Pending && !page.ready(index))
        })
    }

    pub(crate) fn complete_stage(&mut self, page: EntityId, stage: Stage) -> bool {
        let Some(&page_index) = self.page_index.get(&page) else {
            return false;
        };
        let page = &mut self.pages[page_index];
        let was_finished = page.finished();
        if let Some(work) = page.stages.iter_mut().find(|work| work.stage == stage) {
            work.state = WorkState::Finished;
        }
        let page_finished = !was_finished && page.finished();
        if page_finished {
            self.active_pages = self.active_pages.saturating_sub(1);
            while self.head < self.pages.len() && self.pages[self.head].finished() {
                self.head += 1;
            }
        }
        page_finished
    }
}

const fn prerequisite(stage: Stage) -> Option<Stage> {
    match stage {
        Stage::Detection => None,
        Stage::Ocr | Stage::Inpainting => Some(Stage::Detection),
        Stage::Translation => Some(Stage::Ocr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(count: usize) -> Vec<EntityId> {
        (0..count).map(|_| EntityId::new()).collect()
    }

    #[test]
    fn starts_pages_in_order_and_models_independently() {
        let pages = pages(2);
        let mut scheduler = Scheduler::new(&pages, &Stage::ALL);
        let mut busy = BTreeSet::new();

        let first = scheduler.start_next(&busy).unwrap();
        assert_eq!(first, (pages[0], Stage::Detection));
        busy.insert(Stage::Detection);
        assert!(scheduler.start_next(&busy).is_none());

        busy.clear();
        assert!(!scheduler.complete_stage(pages[0], Stage::Detection));
        let ocr = scheduler.start_next(&busy).unwrap();
        busy.insert(ocr.1);
        let inpainting = scheduler.start_next(&busy).unwrap();
        busy.insert(inpainting.1);
        let next_page = scheduler.start_next(&busy).unwrap();
        busy.insert(next_page.1);
        assert_eq!(ocr, (pages[0], Stage::Ocr));
        assert_eq!(inpainting, (pages[0], Stage::Inpainting));
        assert_eq!(next_page, (pages[1], Stage::Detection));

        assert!(!scheduler.complete_stage(pages[0], Stage::Ocr));
        busy.remove(&Stage::Ocr);
        let translation = scheduler.start_next(&busy).unwrap();
        assert_eq!(translation, (pages[0], Stage::Translation));
        assert!(busy.contains(&Stage::Detection));
        assert!(busy.contains(&Stage::Inpainting));
    }

    #[test]
    fn sliding_window_backpressures_fast_upstream_models() {
        let pages = pages(4);
        let stages = [Stage::Detection, Stage::Ocr, Stage::Inpainting];
        let mut scheduler = Scheduler::new(&pages, &stages);
        let mut busy = BTreeSet::new();

        assert_eq!(
            scheduler.start_next(&busy),
            Some((pages[0], Stage::Detection))
        );
        assert!(!scheduler.complete_stage(pages[0], Stage::Detection));
        let ocr = scheduler.start_next(&busy).unwrap();
        busy.insert(ocr.1);
        let inpainting = scheduler.start_next(&busy).unwrap();
        busy.insert(inpainting.1);

        for page in &pages[1..3] {
            assert_eq!(scheduler.start_next(&busy), Some((*page, Stage::Detection)));
            assert!(!scheduler.complete_stage(*page, Stage::Detection));
        }
        assert!(scheduler.start_next(&busy).is_none());

        assert!(!scheduler.complete_stage(pages[0], Stage::Ocr));
        assert!(scheduler.complete_stage(pages[0], Stage::Inpainting));
        busy.clear();
        assert_eq!(scheduler.start_next(&busy), Some((pages[1], Stage::Ocr)));
    }

    #[test]
    fn translation_batches_are_not_overlapped_when_cached_pages_are_scheduled() {
        let pages = pages(5);
        let mut scheduler =
            Scheduler::new(&pages, &[Stage::Translation]).with_translation_batch_pages(4);
        let busy = BTreeSet::new();

        let first = scheduler.start_next_batch(&busy).unwrap();
        assert_eq!(first.page, pages[0]);
        assert_eq!(first.batch_pages, pages[..4]);
        assert!(scheduler.complete_stage(pages[0], Stage::Translation));

        for page in &pages[1..4] {
            let cached = scheduler.start_next_batch(&busy).unwrap();
            assert_eq!(cached.page, *page);
            assert_eq!(cached.batch_pages, [*page]);
            assert!(scheduler.complete_stage(*page, Stage::Translation));
        }

        let tail = scheduler.start_next_batch(&busy).unwrap();
        assert_eq!(tail.page, pages[4]);
        assert_eq!(tail.batch_pages, [pages[4]]);
    }
}
