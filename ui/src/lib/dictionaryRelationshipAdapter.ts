'use client';

import type { DictionaryResolvedRelationship } from '@/lib/dictionaryApi';

export type DictionaryRelationshipCardModel = {
  id: string;
  relationGroupKey: string;
  relationType: string;
  direction: string;
  targetPersonId: string;
  targetPersonName: string;
  targetSummary: string | null;
};

export function toDictionaryRelationshipCardModel(
  relation: DictionaryResolvedRelationship,
): DictionaryRelationshipCardModel {
  return {
    id: relation.relation_id,
    relationGroupKey: relation.relation_group_key,
    relationType: relation.relation_type,
    direction: relation.direction,
    targetPersonId: relation.other_person.id,
    targetPersonName: relation.other_person.display_name,
    targetSummary: relation.other_person.summary,
  };
}

export function sortDictionaryRelationshipCards(
  relations: DictionaryResolvedRelationship[],
): DictionaryRelationshipCardModel[] {
  return relations
    .map(toDictionaryRelationshipCardModel)
    .sort((left, right) => {
      const relationCmp = left.relationType.localeCompare(right.relationType);
      if (relationCmp !== 0) return relationCmp;
      return left.targetPersonName.localeCompare(right.targetPersonName);
    });
}
